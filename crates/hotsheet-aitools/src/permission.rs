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

use hotsheet_sync::LockExt;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Eventual safe-deny guard for an unattended permission request. Client automation is
/// deliberately independent: its countdown advances only while the popup is visible, so
/// this transport guard must be long enough to survive an ignored popup or a 60-minute
/// client setting without silently deciding first.
pub const DEFAULT_PERMISSION_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

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
    /// Remembered for this bridge's lifetime (a "session"), for the whole project.
    Session,
    /// Remembered in memory for the raising connection only — "Allow for this chat"
    /// (HS2-JNHAYB). It outranks a project rule for that connection and ends with it.
    Chat,
    /// Remembered durably (the caller persists the returned rule).
    Always,
}

/// A pending permission request the host must answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Request {
    /// Stable per-bridge id, used to resolve out of order.
    pub id: u64,
    /// Stable project identity. Empty only for legacy single-project callers.
    pub project: String,
    /// The connection that raised it — the route-back target for the answer.
    pub connection: String,
    /// The tool/action asking (e.g. `"Bash"`, `"Edit"`).
    pub tool: String,
    /// What it wants to do (the command / capability / path) — the rule-match key.
    pub action: String,
    /// Asking agent, when the transport identifies it (distinct from tool name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

/// The transport's identity and action before a pending id is assigned.
pub struct PermissionAsk {
    pub project: String,
    pub connection: String,
    pub tool: String,
    pub action: String,
    pub agent: Option<String>,
}

/// A persisted allow-rule: auto-answer any request matching `(project, tool, action)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub project: String,
    pub tool: String,
    pub action: String,
    pub decision: Decision,
    /// `true` for an `Always` rule the caller should persist; `false` for a `Session`
    /// or `Chat` rule that lives only in memory.
    pub persist: bool,
    /// `Some(connection)` for a chat rule (HS2-JNHAYB) that answers only that connection's
    /// requests; `None` for a project-wide rule.
    pub connection: Option<String>,
}

impl Rule {
    fn matches(&self, project: &str, tool: &str, action: &str) -> bool {
        self.project == project && self.tool == tool && self.action == action
    }

    /// Whether two rules occupy the same slot: a newer one replaces an older one.
    fn same_key(&self, other: &Rule) -> bool {
        self.matches(&other.project, &other.tool, &other.action)
            && self.connection == other.connection
    }
}

/// The rule answering a request, by precedence (HS2-JNHAYB): the newest chat rule for the
/// raising connection, else the newest project rule, else none (ask a human).
fn effective_rule<'a>(
    rules: &'a [Rule],
    project: &str,
    connection: &str,
    tool: &str,
    action: &str,
) -> Option<&'a Rule> {
    rules
        .iter()
        .rev()
        .find(|rule| {
            rule.connection.as_deref() == Some(connection) && rule.matches(project, tool, action)
        })
        .or_else(|| {
            rules
                .iter()
                .rev()
                .find(|rule| rule.connection.is_none() && rule.matches(project, tool, action))
        })
}

/// A remembered rule as a client lists it (HS2-JNHAYB): which scope holds it and what it
/// answers. `scope` is `chat`, `session`, or `always`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleInfo {
    pub project: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    pub tool: String,
    pub action: String,
    pub decision: Decision,
    pub scope: Scope,
}

impl From<&Rule> for RuleInfo {
    fn from(rule: &Rule) -> Self {
        Self {
            project: rule.project.clone(),
            connection: rule.connection.clone(),
            tool: rule.tool.clone(),
            action: rule.action.clone(),
            decision: rule.decision,
            scope: if rule.connection.is_some() {
                Scope::Chat
            } else if rule.persist {
                Scope::Always
            } else {
                Scope::Session
            },
        }
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
    pub project: String,
    pub connection: String,
    pub decision: Decision,
    /// A rule to persist, when the answer's scope was `Always`. `None` otherwise.
    pub persisted_rule: Option<Rule>,
    /// Other queued requests the new rule now answers (HS2-JNHAYB): identical asks that were
    /// waiting are resolved with it instead of prompting again.
    pub cascaded: Vec<CascadedResolution>,
}

/// A queued request resolved because a newly remembered rule answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CascadedResolution {
    pub id: u64,
    pub project: String,
    pub connection: String,
    pub decision: Decision,
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
        self.request_for_project("", connection, tool, action)
    }

    /// Raise a permission request scoped to one project.
    pub fn request_for_project(
        &mut self,
        project: impl Into<String>,
        connection: impl Into<String>,
        tool: impl Into<String>,
        action: impl Into<String>,
    ) -> Outcome {
        self.request_for_project_as(project, connection, tool, action, None)
    }

    pub fn request_for_project_as(
        &mut self,
        project: impl Into<String>,
        connection: impl Into<String>,
        tool: impl Into<String>,
        action: impl Into<String>,
        agent: Option<String>,
    ) -> Outcome {
        let project = project.into();
        let tool = tool.into();
        let action = action.into();
        let connection = connection.into();
        if let Some(rule) = effective_rule(&self.rules, &project, &connection, &tool, &action) {
            return Outcome::Auto(rule.decision);
        }
        self.next_id += 1;
        let id = self.next_id;
        self.queue.push_back(Request {
            id,
            project,
            connection,
            tool,
            action,
            agent,
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
    /// Chat rules are in-memory state, not storage, so they survive a reseed.
    pub fn set_rules(&mut self, rules: Vec<Rule>) {
        let chat: Vec<Rule> = self
            .rules
            .drain(..)
            .filter(|rule| rule.connection.is_some())
            .collect();
        self.rules = rules;
        self.rules.extend(chat);
    }

    /// Forget one remembered rule (HS2-JNHAYB: revoke from settings). Matches the exact
    /// slot — project, chat connection (or none), tool, action. Returns the removed rule.
    pub fn revoke(
        &mut self,
        project: &str,
        connection: Option<&str>,
        tool: &str,
        action: &str,
    ) -> Option<Rule> {
        let position = self.rules.iter().position(|rule| {
            rule.matches(project, tool, action) && rule.connection.as_deref() == connection
        })?;
        Some(self.rules.remove(position))
    }

    /// Drop every chat rule of a connection when its chat closes (HS2-JNHAYB).
    pub fn forget_connection(&mut self, connection: &str) -> usize {
        let before = self.rules.len();
        self.rules
            .retain(|rule| rule.connection.as_deref() != Some(connection));
        before - self.rules.len()
    }

    /// Answer a pending request by id. Removes it from the queue (position-independent,
    /// so answering the middle of the queue leaves the rest intact), records a
    /// session/always rule per `scope`, and returns who to route the answer back to.
    /// `None` if no pending request has that id (already answered / never existed).
    pub fn resolve(&mut self, id: u64, decision: Decision, scope: Scope) -> Option<Resolved> {
        let pos = self.queue.iter().position(|r| r.id == id)?;
        let req = self.queue.remove(pos).expect("position just found");

        let mut persisted_rule = None;
        let mut cascaded = Vec::new();
        if scope != Scope::Once {
            let rule = Rule {
                project: req.project.clone(),
                tool: req.tool.clone(),
                action: req.action.clone(),
                decision,
                persist: scope == Scope::Always,
                connection: (scope == Scope::Chat).then(|| req.connection.clone()),
            };
            // A later rule for the same key (and the same chat, for a chat rule) supersedes
            // an earlier one; chat and project rules coexist and precedence picks between them.
            self.rules.retain(|r| !r.same_key(&rule));
            if scope == Scope::Always {
                persisted_rule = Some(rule.clone());
            }
            self.rules.push(rule);
            // Identical asks already waiting are answered by the new rule now (HS2-JNHAYB).
            let rules = &self.rules;
            let (answered, waiting): (Vec<Request>, Vec<Request>) =
                self.queue.drain(..).partition(|queued| {
                    effective_rule(
                        rules,
                        &queued.project,
                        &queued.connection,
                        &queued.tool,
                        &queued.action,
                    )
                    .is_some()
                });
            self.queue = waiting.into();
            for queued in answered {
                let decision = effective_rule(
                    &self.rules,
                    &queued.project,
                    &queued.connection,
                    &queued.tool,
                    &queued.action,
                )
                .map(|rule| rule.decision)
                .expect("partitioned by a matching rule");
                cascaded.push(CascadedResolution {
                    id: queued.id,
                    project: queued.project,
                    connection: queued.connection,
                    decision,
                });
            }
        }

        Some(Resolved {
            project: req.project,
            connection: req.connection,
            decision,
            persisted_rule,
            cascaded,
        })
    }
}

/// An observer of a permission request entering or leaving the queue.
type RequestCallback = Box<dyn Fn(&Request) + Send + Sync>;

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
    /// Async counterpart of `cvar`: every `resolve`/`cancel` also wakes the server-side
    /// awaiting asks, so a pending `/permissions/ask` parks no thread (HS2-43YT58).
    async_notify: tokio::sync::Notify,
    /// Fired when a request is enqueued (goes `Pending`) — the server sets this to push a
    /// "permission_asked" nudge over its event bus so attached clients fetch + answer it
    /// (HS2-9R9YZW). `None` = headless / no observer.
    on_pending: Mutex<Option<RequestCallback>>,
    on_removed: Mutex<Option<RequestCallback>>,
}

impl SharedPermissionBridge {
    pub fn new(bridge: PermissionBridge) -> Self {
        Self {
            inner: Mutex::new(bridge),
            results: Mutex::new(HashMap::new()),
            cvar: Condvar::new(),
            async_notify: tokio::sync::Notify::new(),
            on_pending: Mutex::new(None),
            on_removed: Mutex::new(None),
        }
    }

    /// Register the enqueue observer (the server's WS/event-bus nudge). Replaces any prior.
    pub fn set_on_pending(&self, f: impl Fn(&Request) + Send + Sync + 'static) {
        *self.on_pending.lock_or_recover() = Some(Box::new(f));
    }

    /// Notify clients when cancellation or timeout removes an unanswered approval.
    pub fn set_on_removed(&self, f: impl Fn(&Request) + Send + Sync + 'static) {
        *self.on_removed.lock_or_recover() = Some(Box::new(f));
    }

    /// Replace the bridge's allow-rules (e.g. seeding durable `Always` rules loaded from
    /// disk at startup) without disturbing the observer or any in-flight waiters.
    pub fn reseed_rules(&self, rules: Vec<Rule>) {
        self.inner.lock_or_recover().set_rules(rules);
    }

    /// Add another project's durable rules without disturbing already seeded projects.
    pub fn add_rules(&self, rules: impl IntoIterator<Item = Rule>) {
        self.inner.lock_or_recover().rules.extend(rules);
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
        self.request_blocking_for_project("", connection, tool, action)
    }

    pub fn request_blocking_for_project(
        &self,
        project: impl Into<String>,
        connection: impl Into<String>,
        tool: impl Into<String>,
        action: impl Into<String>,
    ) -> Decision {
        // Materialize the fields up front so the pending observer gets the full Request.
        let (project, connection, tool, action) = (
            project.into(),
            connection.into(),
            tool.into(),
            action.into(),
        );
        let id = match self.inner.lock_or_recover().request_for_project(
            project.clone(),
            connection.clone(),
            tool.clone(),
            action.clone(),
        ) {
            Outcome::Auto(d) => return d, // a rule answered — no human needed
            Outcome::Pending(id) => id,
        };
        // Nudge any observer (server → WS "permission_asked") now that it's queued.
        if let Some(f) = self.on_pending.lock_or_recover().as_ref() {
            f(&Request {
                id,
                project,
                connection,
                tool,
                action,
                agent: None,
            });
        }
        // Wait for the answer. If `resolve` already ran (before we started waiting), the
        // result is already present, so there's no lost wakeup.
        let mut results = self.results.lock_or_recover();
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
        self.request_blocking_timeout_for_project("", connection, tool, action, timeout, on_timeout)
    }

    pub fn request_blocking_timeout_for_project(
        &self,
        project: impl Into<String>,
        connection: impl Into<String>,
        tool: impl Into<String>,
        action: impl Into<String>,
        timeout: Duration,
        on_timeout: Decision,
    ) -> Decision {
        self.request_blocking_timeout_with_pending(
            PermissionAsk {
                project: project.into(),
                connection: connection.into(),
                tool: tool.into(),
                action: action.into(),
                agent: None,
            },
            timeout,
            on_timeout,
            |_| {},
            |_| {},
        )
    }

    pub fn request_blocking_timeout_with_pending(
        &self,
        ask: PermissionAsk,
        timeout: Duration,
        on_timeout: Decision,
        on_pending: impl FnOnce(u64),
        on_expired: impl FnOnce(u64),
    ) -> Decision {
        let request = match self.enqueue(ask) {
            Enqueued::Answered(decision) => return decision,
            Enqueued::Pending(request) => request,
        };
        let id = request.id;
        on_pending(id);
        let deadline = Instant::now() + timeout;
        let mut results = self.results.lock_or_recover();
        loop {
            if let Some(d) = results.remove(&id) {
                return d;
            }
            let now = Instant::now();
            if now >= deadline {
                drop(results);
                return self.expire(&request, on_timeout, on_expired);
            }
            let (guard, _) = self.cvar.wait_timeout(results, deadline - now).unwrap();
            results = guard;
        }
    }

    /// The async form of [`request_blocking_timeout_with_pending`] for a server-side ask
    /// (`POST /permissions/ask`): the wait parks **no thread**, not even a blocking-pool
    /// one, so any number of pending asks cannot starve offloaded API work (HS2-43YT58).
    /// Dropping the future before it settles withdraws the ask as a one-time denial (the
    /// asking hook went away), firing the removed observer, and leaves no result behind.
    ///
    /// [`request_blocking_timeout_with_pending`]: Self::request_blocking_timeout_with_pending
    pub async fn request_timeout_with_pending_async(
        &self,
        ask: PermissionAsk,
        timeout: Duration,
        on_timeout: Decision,
        on_pending: impl FnOnce(u64),
        on_expired: impl FnOnce(u64),
    ) -> Decision {
        let request = match self.enqueue(ask) {
            Enqueued::Answered(decision) => return decision,
            Enqueued::Pending(request) => request,
        };
        let mut waiter = AsyncAsk {
            bridge: self,
            id: request.id,
            settled: false,
        };
        on_pending(request.id);
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            // Register for the wakeup *before* checking, so a resolve landing between the
            // check and the await is never lost.
            let notified = self.async_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let answered = self.results.lock_or_recover().remove(&request.id);
            if let Some(d) = answered {
                waiter.settled = true;
                return d;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                waiter.settled = true;
                return self.expire(&request, on_timeout, on_expired);
            }
        }
    }

    /// Apply the allow-rules; an unanswered ask is enqueued (firing the pending observer)
    /// and returned for the caller to wait on.
    fn enqueue(&self, ask: PermissionAsk) -> Enqueued {
        let PermissionAsk {
            project,
            connection,
            tool,
            action,
            agent,
        } = ask;
        let id = match self.inner.lock_or_recover().request_for_project_as(
            project.clone(),
            connection.clone(),
            tool.clone(),
            action.clone(),
            agent.clone(),
        ) {
            Outcome::Auto(d) => return Enqueued::Answered(d),
            Outcome::Pending(id) => id,
        };
        let request = Request {
            id,
            project,
            connection,
            tool,
            action,
            agent,
        };
        if let Some(f) = self.on_pending.lock_or_recover().as_ref() {
            f(&request);
        }
        Enqueued::Pending(request)
    }

    /// Settle a timed-out ask: win the race to abandon it, or honor a human answer that
    /// landed just now.
    fn expire(
        &self,
        request: &Request,
        on_timeout: Decision,
        on_expired: impl FnOnce(u64),
    ) -> Decision {
        let expired = self
            .inner
            .lock_or_recover()
            .resolve(request.id, on_timeout, Scope::Once)
            .is_some();
        if expired {
            on_expired(request.id);
            self.notify_removed(request);
            return on_timeout;
        }
        self.results
            .lock_or_recover()
            .remove(&request.id)
            .unwrap_or(on_timeout)
    }

    fn wake_waiters(&self) {
        self.cvar.notify_all();
        self.async_notify.notify_waiters();
    }

    /// The pending requests (for the server to push over the WS to clients).
    pub fn pending(&self) -> Vec<Request> {
        self.inner.lock_or_recover().pending().cloned().collect()
    }

    /// Answer a pending request (the route-back a client POSTs). Wakes the blocked waiter and
    /// returns who raised it + any rule to persist. `None` if the id isn't pending.
    pub fn resolve(&self, id: u64, decision: Decision, scope: Scope) -> Option<Resolved> {
        let resolved = self.inner.lock_or_recover().resolve(id, decision, scope)?;
        {
            let mut results = self.results.lock_or_recover();
            results.insert(id, decision);
            // Identical waiting asks the new rule answered wake with its decision too.
            for cascaded in &resolved.cascaded {
                results.insert(cascaded.id, cascaded.decision);
            }
        }
        self.wake_waiters();
        Some(resolved)
    }

    /// Every remembered rule, newest last (HS2-JNHAYB settings list).
    pub fn rules(&self) -> Vec<RuleInfo> {
        self.inner
            .lock_or_recover()
            .rules()
            .iter()
            .map(RuleInfo::from)
            .collect()
    }

    /// Forget one remembered rule; see [`PermissionBridge::revoke`].
    pub fn revoke(
        &self,
        project: &str,
        connection: Option<&str>,
        tool: &str,
        action: &str,
    ) -> Option<Rule> {
        self.inner
            .lock_or_recover()
            .revoke(project, connection, tool, action)
    }

    /// Drop a closed chat's rules; see [`PermissionBridge::forget_connection`].
    pub fn forget_connection(&self, connection: &str) -> usize {
        self.inner.lock_or_recover().forget_connection(connection)
    }

    /// Withdraw an approval whose asking transport can no longer accept a decision.
    /// This wakes its waiter with a one-time denial and never creates a durable rule.
    pub fn cancel(&self, id: u64) -> bool {
        let request = {
            let mut bridge = self.inner.lock_or_recover();
            let Some(request) = bridge.pending().find(|request| request.id == id).cloned() else {
                return false;
            };
            bridge.resolve(id, Decision::Deny, Scope::Once);
            request
        };
        self.results.lock_or_recover().insert(id, Decision::Deny);
        self.wake_waiters();
        self.notify_removed(&request);
        true
    }

    fn notify_removed(&self, request: &Request) {
        if let Some(callback) = self.on_removed.lock_or_recover().as_ref() {
            callback(request);
        }
    }
}

/// Whether an ask was answered by a rule or is now queued for a human.
enum Enqueued {
    Answered(Decision),
    Pending(Request),
}

/// An in-flight async ask. Dropped unsettled (its caller went away), it withdraws the
/// pending request and clears any decision delivered for it so nothing leaks.
struct AsyncAsk<'a> {
    bridge: &'a SharedPermissionBridge,
    id: u64,
    settled: bool,
}

impl Drop for AsyncAsk<'_> {
    fn drop(&mut self) {
        if !self.settled {
            self.bridge.cancel(self.id);
            self.bridge.results.lock_or_recover().remove(&self.id);
        }
    }
}

// ---- durable allow-rule storage (HS2-9R9YZW) -------------------------------------

/// The on-disk form of an `Always` allow-rule — enough to auto-answer the same
/// `(project, tool, action)` on a later run. The transient `persist` flag isn't stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRule {
    #[serde(default)]
    pub project: String,
    pub tool: String,
    pub action: String,
    pub decision: Decision,
}

impl From<&Rule> for StoredRule {
    fn from(r: &Rule) -> Self {
        StoredRule {
            project: r.project.clone(),
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
            project: self.project,
            tool: self.tool,
            action: self.action,
            decision: self.decision,
            persist: true,
            connection: None,
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

/// Remove a durable rule for `(project, tool, action)` from `path` (HS2-JNHAYB revoke).
/// Returns whether one was removed; a missing file removes nothing.
pub fn remove_rule(path: &Path, project: &str, tool: &str, action: &str) -> std::io::Result<bool> {
    let mut rules: Vec<StoredRule> = load_rules(path).iter().map(StoredRule::from).collect();
    let before = rules.len();
    // Durable rules may be stored with the project or with the legacy empty project.
    rules.retain(|r| {
        !((r.project == project || r.project.is_empty()) && r.tool == tool && r.action == action)
    });
    if rules.len() == before {
        return Ok(false);
    }
    let json = serde_json::to_string_pretty(&rules).map_err(std::io::Error::other)?;
    std::fs::write(path, json)?;
    Ok(true)
}

/// Persist an `Always` allow-rule to `path`, replacing any existing rule for the same
/// `(project, tool, action)` (a later answer wins) — so remembered answers survive a restart.
pub fn append_rule(path: &Path, rule: &Rule) -> std::io::Result<()> {
    let mut rules: Vec<StoredRule> = load_rules(path).iter().map(StoredRule::from).collect();
    rules
        .retain(|r| !(r.project == rule.project && r.tool == rule.tool && r.action == rule.action));
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
    fn a_poisoned_bridge_lock_keeps_serving_requests() {
        // HS2-B2RFSB: recover a poisoned lock instead of panicking every later caller.
        use std::sync::Arc;
        let b = Arc::new(SharedPermissionBridge::new(PermissionBridge::new()));
        let poisoner = b.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poison the bridge");
        })
        .join();
        assert!(b.inner.is_poisoned());
        assert!(b.pending().is_empty());
    }

    #[test]
    fn default_transport_timeout_outlasts_longest_client_automation_window() {
        assert!(DEFAULT_PERMISSION_TIMEOUT > Duration::from_secs(60 * 60));
        assert_eq!(
            DEFAULT_PERMISSION_TIMEOUT,
            Duration::from_secs(24 * 60 * 60)
        );
    }

    #[test]
    fn shared_bridge_blocks_until_a_human_resolves() {
        use std::sync::Arc;
        // A seeded rule auto-resolves with no blocking.
        let b = Arc::new(SharedPermissionBridge::new(PermissionBridge::with_rules(
            vec![Rule {
                project: String::new(),
                tool: "Bash".into(),
                action: "ls".into(),
                decision: Decision::Allow,
                persist: true,
                connection: None,
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
    fn cancelling_one_transport_request_wakes_only_its_waiter_and_persists_no_rule() {
        use std::sync::Arc;
        let bridge = Arc::new(SharedPermissionBridge::default());
        let cancelled = Arc::new(Mutex::new(Vec::new()));
        let observed = cancelled.clone();
        bridge.set_on_removed(move |request| observed.lock_or_recover().push(request.id));
        let first_bridge = bridge.clone();
        let first =
            std::thread::spawn(move || first_bridge.request_blocking("first", "Bash", "rm x"));
        let second_bridge = bridge.clone();
        let second =
            std::thread::spawn(move || second_bridge.request_blocking("second", "Bash", "rm x"));
        let deadline = Instant::now() + Duration::from_secs(2);
        let (first_id, second_id) = loop {
            let pending = bridge.pending();
            let first = pending.iter().find(|request| request.connection == "first");
            let second = pending
                .iter()
                .find(|request| request.connection == "second");
            if let (Some(first), Some(second)) = (first, second) {
                break (first.id, second.id);
            }
            assert!(
                Instant::now() < deadline,
                "both approvals should enter the queue"
            );
            std::thread::yield_now();
        };
        assert!(bridge.cancel(first_id));
        assert!(!bridge.cancel(first_id));
        assert!(
            bridge
                .resolve(first_id, Decision::Allow, Scope::Always)
                .is_none()
        );
        assert_eq!(first.join().unwrap(), Decision::Deny);
        assert_eq!(
            bridge
                .pending()
                .iter()
                .map(|request| request.id)
                .collect::<Vec<_>>(),
            vec![second_id]
        );
        assert_eq!(*cancelled.lock_or_recover(), vec![first_id]);
        bridge
            .resolve(second_id, Decision::Allow, Scope::Once)
            .unwrap();
        assert_eq!(second.join().unwrap(), Decision::Allow);
        assert!(bridge.inner.lock_or_recover().rules().is_empty());
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
    fn expiry_callback_runs_only_when_an_unanswered_request_leaves_the_queue() {
        use std::sync::{Arc, Mutex};
        let bridge = Arc::new(SharedPermissionBridge::default());
        let expired = Arc::new(Mutex::new(Vec::new()));
        let removed = Arc::new(Mutex::new(Vec::new()));
        let observed = removed.clone();
        bridge.set_on_removed(move |request| observed.lock_or_recover().push(request.id));
        let timeout_ids = expired.clone();
        let decision = bridge.request_blocking_timeout_with_pending(
            PermissionAsk {
                project: "p".into(),
                connection: "c".into(),
                tool: "Bash".into(),
                action: "timeout".into(),
                agent: None,
            },
            Duration::from_millis(1),
            Decision::Deny,
            |_| {},
            move |id| timeout_ids.lock_or_recover().push(id),
        );
        assert_eq!(decision, Decision::Deny);
        assert_eq!(expired.lock_or_recover().len(), 1);
        assert_eq!(*removed.lock_or_recover(), *expired.lock_or_recover());
        assert!(bridge.pending().is_empty());

        let waiter_bridge = bridge.clone();
        let human_ids = expired.clone();
        let waiter = std::thread::spawn(move || {
            waiter_bridge.request_blocking_timeout_with_pending(
                PermissionAsk {
                    project: "p".into(),
                    connection: "c".into(),
                    tool: "Bash".into(),
                    action: "human".into(),
                    agent: None,
                },
                Duration::from_secs(1),
                Decision::Deny,
                |_| {},
                move |id| human_ids.lock_or_recover().push(id),
            )
        });
        let id = loop {
            if let Some(request) = bridge.pending().into_iter().next() {
                break request.id;
            }
            std::thread::yield_now();
        };
        bridge.resolve(id, Decision::Allow, Scope::Once).unwrap();
        assert_eq!(waiter.join().unwrap(), Decision::Allow);
        assert_eq!(expired.lock_or_recover().len(), 1);
        assert_eq!(removed.lock_or_recover().len(), 1);
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
                project: String::new(),
                tool: "Bash".into(),
                action: "ls".into(),
                decision: Decision::Allow,
                persist: true,
                connection: None,
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
                project: String::new(),
                tool: "Bash".into(),
                action: "ls".into(),
                decision: Decision::Allow,
                persist: true,
                connection: None,
            }],
        )));
        let seen = Arc::new(Mutex::new(Vec::<(u64, String, String)>::new()));
        let s2 = seen.clone();
        b.set_on_pending(move |r| {
            s2.lock_or_recover()
                .push((r.id, r.tool.clone(), r.action.clone()));
        });

        // A rule-answered request never enqueues → no notification.
        assert_eq!(b.request_blocking("c", "Bash", "ls"), Decision::Allow);
        assert!(seen.lock_or_recover().is_empty(), "auto-resolved: no nudge");

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
        let got = seen.lock_or_recover().clone();
        assert_eq!(got, vec![(id, "Bash".to_string(), "rm y".to_string())]);
    }

    #[test]
    fn durable_rules_round_trip_and_dedupe_by_tool_action() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("permissions.json");
        assert!(load_rules(&path).is_empty(), "missing file → no rules");

        let allow = Rule {
            project: String::new(),
            tool: "Bash".into(),
            action: "ls".into(),
            decision: Decision::Allow,
            persist: true,
            connection: None,
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
    fn rules_never_cross_project_boundaries() {
        let mut bridge = PermissionBridge::new();
        let first = match bridge.request_for_project("project-a", "c1", "Bash", "npm test") {
            Outcome::Pending(id) => id,
            _ => panic!("first project must ask"),
        };
        bridge
            .resolve(first, Decision::Allow, Scope::Session)
            .unwrap();
        assert_eq!(
            bridge.request_for_project("project-a", "c2", "Bash", "npm test"),
            Outcome::Auto(Decision::Allow)
        );
        assert!(matches!(
            bridge.request_for_project("project-b", "c3", "Bash", "npm test"),
            Outcome::Pending(_)
        ));
        assert_eq!(bridge.rules()[0].project, "project-a");
    }

    #[test]
    fn seeded_rules_auto_resolve_from_the_first_request() {
        let mut b = PermissionBridge::with_rules(vec![Rule {
            project: String::new(),
            tool: "Bash".into(),
            action: "git status".into(),
            decision: Decision::Allow,
            persist: true,
            connection: None,
        }]);
        assert_eq!(
            b.request("c", "Bash", "git status"),
            Outcome::Auto(Decision::Allow),
            "a persisted rule loaded at startup answers immediately"
        );
    }

    #[test]
    fn a_remembered_answer_resolves_identical_waiting_asks() {
        // Two requests for the SAME key both enqueue (no rule exists yet at request time);
        // remembering the first answer resolves the second with it (HS2-JNHAYB) instead of
        // prompting again.
        let mut b = PermissionBridge::new();
        let (id1, id2) = match (
            b.request("c", "Bash", "curl x"),
            b.request("c", "Bash", "curl x"),
        ) {
            (Outcome::Pending(a), Outcome::Pending(b)) => (a, b),
            _ => panic!("both enqueue — no rule yet"),
        };
        let resolved = b.resolve(id1, Decision::Allow, Scope::Session).unwrap();
        assert_eq!(resolved.cascaded.len(), 1);
        assert_eq!(resolved.cascaded[0].id, id2);
        assert_eq!(resolved.cascaded[0].decision, Decision::Allow);
        assert!(b.resolve(id2, Decision::Deny, Scope::Session).is_none());
        assert_eq!(b.pending_count(), 0);
        assert_eq!(b.rules().len(), 1, "one rule per (tool, action) key");
        // A one-time answer never cascades.
        let (a, c) = match (
            b.request("c", "Bash", "rm x"),
            b.request("c", "Bash", "rm x"),
        ) {
            (Outcome::Pending(a), Outcome::Pending(c)) => (a, c),
            _ => panic!("both enqueue"),
        };
        assert!(
            b.resolve(a, Decision::Allow, Scope::Once)
                .unwrap()
                .cascaded
                .is_empty()
        );
        assert_eq!(b.pending().map(|r| r.id).collect::<Vec<_>>(), [c]);
    }

    fn pending(outcome: Outcome) -> u64 {
        match outcome {
            Outcome::Pending(id) => id,
            Outcome::Auto(decision) => panic!("expected a prompt, got {decision:?}"),
        }
    }

    /// HS2-JNHAYB transition matrix for rule precedence: chat > project > ask, across
    /// chat-then-project, project-then-chat, revoke, and chat close.
    #[test]
    fn chat_rules_outrank_project_rules_which_outrank_asking() {
        let mut b = PermissionBridge::new();
        // ask → chat allow (only that chat).
        let id = pending(b.request_for_project("p", "chat-a", "Bash", "ls"));
        let resolved = b.resolve(id, Decision::Allow, Scope::Chat).unwrap();
        assert!(
            resolved.persisted_rule.is_none(),
            "a chat rule is never persisted"
        );
        assert_eq!(
            b.request_for_project("p", "chat-a", "Bash", "ls"),
            Outcome::Auto(Decision::Allow)
        );
        let other = pending(b.request_for_project("p", "chat-b", "Bash", "ls"));
        assert!(
            matches!(
                b.request_for_project("q", "chat-a", "Bash", "ls"),
                Outcome::Pending(_)
            ),
            "project-scoped"
        );
        // chat-b denies for the whole project → chat-a's own allow still wins for chat-a.
        b.resolve(other, Decision::Deny, Scope::Always).unwrap();
        assert_eq!(
            b.request_for_project("p", "chat-a", "Bash", "ls"),
            Outcome::Auto(Decision::Allow)
        );
        assert_eq!(
            b.request_for_project("p", "chat-c", "Bash", "ls"),
            Outcome::Auto(Decision::Deny)
        );
        // A chat answer added after a project rule still wins for its chat (project then chat).
        let infos: Vec<RuleInfo> = b.rules().iter().map(RuleInfo::from).collect();
        assert_eq!(
            infos.iter().map(|rule| rule.scope).collect::<Vec<_>>(),
            [Scope::Chat, Scope::Always]
        );
        // Revoking the chat rule falls back to the project rule; revoking that asks again.
        assert!(b.revoke("p", Some("chat-a"), "Bash", "ls").is_some());
        assert_eq!(
            b.request_for_project("p", "chat-a", "Bash", "ls"),
            Outcome::Auto(Decision::Deny)
        );
        assert!(
            b.revoke("p", Some("chat-a"), "Bash", "ls").is_none(),
            "already gone"
        );
        assert!(b.revoke("p", None, "Bash", "ls").is_some());
        let asked = pending(b.request_for_project("p", "chat-a", "Bash", "ls"));
        // Empty → refill: a fresh chat rule, then closing the chat forgets it.
        b.resolve(asked, Decision::Deny, Scope::Chat).unwrap();
        assert_eq!(
            b.request_for_project("p", "chat-a", "Bash", "ls"),
            Outcome::Auto(Decision::Deny)
        );
        assert_eq!(b.forget_connection("chat-a"), 1);
        assert!(matches!(
            b.request_for_project("p", "chat-a", "Bash", "ls"),
            Outcome::Pending(_)
        ));
    }

    #[test]
    fn a_chat_answer_cascades_only_to_that_chat_and_a_project_answer_respects_chat_rules() {
        let mut b = PermissionBridge::new();
        let a1 = pending(b.request_for_project("p", "chat-a", "Bash", "ls"));
        let a2 = pending(b.request_for_project("p", "chat-a", "Bash", "ls"));
        let b1 = pending(b.request_for_project("p", "chat-b", "Bash", "ls"));
        let resolved = b.resolve(a1, Decision::Allow, Scope::Chat).unwrap();
        assert_eq!(
            resolved.cascaded.iter().map(|c| c.id).collect::<Vec<_>>(),
            [a2]
        );
        assert_eq!(b.pending().map(|r| r.id).collect::<Vec<_>>(), [b1]);
        // chat-a now has a chat allow; a project deny resolves chat-b's wait, and chat-a keeps allow.
        let a3 = {
            // A chat-a ask can't be pending (its chat rule answers), so add a third chat.
            pending(b.request_for_project("p", "chat-c", "Bash", "ls"))
        };
        let resolved = b.resolve(b1, Decision::Deny, Scope::Session).unwrap();
        assert_eq!(
            resolved
                .cascaded
                .iter()
                .map(|c| (c.id, c.decision))
                .collect::<Vec<_>>(),
            [(a3, Decision::Deny)]
        );
        assert_eq!(
            b.request_for_project("p", "chat-a", "Bash", "ls"),
            Outcome::Auto(Decision::Allow)
        );
    }

    #[test]
    fn reseeding_durable_rules_keeps_chat_rules_and_revoked_durable_rules_leave_the_file() {
        let mut b = PermissionBridge::new();
        let id = pending(b.request_for_project("p", "chat-a", "Bash", "ls"));
        b.resolve(id, Decision::Allow, Scope::Chat).unwrap();
        b.set_rules(Vec::new());
        assert_eq!(
            b.request_for_project("p", "chat-a", "Bash", "ls"),
            Outcome::Auto(Decision::Allow)
        );

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");
        let rule = Rule {
            project: "p".into(),
            tool: "Bash".into(),
            action: "ls".into(),
            decision: Decision::Allow,
            persist: true,
            connection: None,
        };
        append_rule(&path, &rule).unwrap();
        append_rule(
            &path,
            &Rule {
                action: "pwd".into(),
                ..rule.clone()
            },
        )
        .unwrap();
        assert!(remove_rule(&path, "p", "Bash", "ls").unwrap());
        assert!(
            !remove_rule(&path, "p", "Bash", "ls").unwrap(),
            "already removed"
        );
        assert_eq!(load_rules(&path).len(), 1);
        assert!(!remove_rule(&dir.path().join("missing.json"), "p", "Bash", "ls").unwrap());
    }

    fn ask(action: &str) -> PermissionAsk {
        PermissionAsk {
            project: "p".into(),
            connection: "c".into(),
            tool: "Bash".into(),
            action: action.into(),
            agent: None,
        }
    }

    async fn first_pending(bridge: &SharedPermissionBridge, count: usize) -> Vec<u64> {
        loop {
            let pending = bridge.pending();
            if pending.len() >= count {
                return pending.into_iter().map(|request| request.id).collect();
            }
            tokio::task::yield_now().await;
        }
    }

    // HS2-43YT58: the async server-side ask parks no thread and keeps the sync semantics.
    #[tokio::test(flavor = "current_thread")]
    async fn async_ask_waits_without_a_thread_and_wakes_on_resolve() {
        use std::sync::Arc;
        let bridge = Arc::new(SharedPermissionBridge::default());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let marked = seen.clone();
        let waiter = bridge.clone();
        // A current-thread runtime with no blocking work: the ask can only make progress if
        // its wait is a real async wait.
        let task = tokio::spawn(async move {
            waiter
                .request_timeout_with_pending_async(
                    ask("rm x"),
                    Duration::from_secs(60),
                    Decision::Deny,
                    move |id| marked.lock_or_recover().push(id),
                    |_| panic!("answered asks never expire"),
                )
                .await
        });
        let ids = first_pending(&bridge, 1).await;
        assert_eq!(
            *seen.lock_or_recover(),
            ids,
            "on_pending names the queued id"
        );
        bridge
            .resolve(ids[0], Decision::Allow, Scope::Once)
            .unwrap();
        assert_eq!(task.await.unwrap(), Decision::Allow);
        assert!(bridge.pending().is_empty());
        assert!(
            bridge.results.lock_or_recover().is_empty(),
            "no result leaks"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn async_ask_honors_rules_timeouts_and_resolves_before_the_await() {
        let ruled = SharedPermissionBridge::new(PermissionBridge::with_rules(vec![Rule {
            project: "p".into(),
            tool: "Bash".into(),
            action: "ls".into(),
            decision: Decision::Allow,
            persist: false,
            connection: None,
        }]));
        let decision = ruled
            .request_timeout_with_pending_async(
                ask("ls"),
                Duration::from_secs(60),
                Decision::Deny,
                |_| panic!("a ruled ask is never pending"),
                |_| panic!("a ruled ask never expires"),
            )
            .await;
        assert_eq!(decision, Decision::Allow);

        let bridge = SharedPermissionBridge::default();
        let removed = std::sync::Arc::new(Mutex::new(0));
        let observed = removed.clone();
        bridge.set_on_removed(move |_| *observed.lock_or_recover() += 1);
        let expired = Mutex::new(Vec::new());
        let decision = bridge
            .request_timeout_with_pending_async(
                ask("slow"),
                Duration::from_millis(5),
                Decision::Deny,
                |_| {},
                |id| expired.lock_or_recover().push(id),
            )
            .await;
        assert_eq!(decision, Decision::Deny, "timed out: the safe fallback");
        assert_eq!(expired.lock_or_recover().len(), 1);
        assert_eq!(*removed.lock_or_recover(), 1);
        assert!(bridge.pending().is_empty());

        // An answer delivered from on_pending (before the first await) is not lost.
        let bridge = std::sync::Arc::new(SharedPermissionBridge::default());
        let answering = bridge.clone();
        let decision = bridge
            .request_timeout_with_pending_async(
                ask("fast"),
                Duration::from_secs(60),
                Decision::Deny,
                move |id| {
                    answering.resolve(id, Decision::Allow, Scope::Once).unwrap();
                },
                |_| panic!("answered asks never expire"),
            )
            .await;
        assert_eq!(decision, Decision::Allow);
        assert!(bridge.results.lock_or_recover().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dropping_an_async_ask_withdraws_it_once_and_leaves_no_result() {
        use std::sync::Arc;
        let bridge = Arc::new(SharedPermissionBridge::default());
        let removed = Arc::new(Mutex::new(Vec::new()));
        let observed = removed.clone();
        bridge.set_on_removed(move |request| observed.lock_or_recover().push(request.id));
        let waiter = bridge.clone();
        let task = tokio::spawn(async move {
            waiter
                .request_timeout_with_pending_async(
                    ask("rm x"),
                    Duration::from_secs(60),
                    Decision::Deny,
                    |_| {},
                    |_| panic!("a withdrawn ask does not expire"),
                )
                .await
        });
        let ids = first_pending(&bridge, 1).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(bridge.pending().is_empty(), "the dropped ask was withdrawn");
        assert_eq!(
            *removed.lock_or_recover(),
            ids,
            "removed observer fired once"
        );
        assert!(bridge.results.lock_or_recover().is_empty());
        // A late answer for the withdrawn id is a no-op.
        assert!(
            bridge
                .resolve(ids[0], Decision::Allow, Scope::Once)
                .is_none()
        );
        assert!(bridge.results.lock_or_recover().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn async_and_blocking_asks_share_cascades_and_cancellation() {
        use std::sync::Arc;
        let bridge = Arc::new(SharedPermissionBridge::default());
        // A tool thread (sync) and two server asks (async) wait on the same (tool, action).
        let sync_bridge = bridge.clone();
        let sync = std::thread::spawn(move || {
            sync_bridge.request_blocking_timeout_with_pending(
                ask("same"),
                Duration::from_secs(60),
                Decision::Deny,
                |_| {},
                |_| {},
            )
        });
        let spawn_async = |action: &'static str| {
            let waiter = bridge.clone();
            tokio::spawn(async move {
                waiter
                    .request_timeout_with_pending_async(
                        ask(action),
                        Duration::from_secs(60),
                        Decision::Deny,
                        |_| {},
                        |_| {},
                    )
                    .await
            })
        };
        let same = spawn_async("same");
        let other = spawn_async("other");
        let ids = first_pending(&bridge, 3).await;
        // Answering one with Always cascades to the identical waiting asks, sync and async.
        let same_id = bridge
            .pending()
            .into_iter()
            .find(|request| request.action == "same")
            .unwrap()
            .id;
        let resolved = bridge
            .resolve(same_id, Decision::Allow, Scope::Always)
            .unwrap();
        assert_eq!(resolved.cascaded.len(), 1);
        assert_eq!(same.await.unwrap(), Decision::Allow);
        assert_eq!(sync.join().unwrap(), Decision::Allow);
        // Cancelling the remaining async ask denies it once.
        let other_id = *ids
            .iter()
            .find(|id| bridge.pending().iter().any(|request| request.id == **id))
            .unwrap();
        assert!(bridge.cancel(other_id));
        assert_eq!(other.await.unwrap(), Decision::Deny);
        assert!(bridge.pending().is_empty());
        assert!(bridge.results.lock_or_recover().is_empty());
    }
}
