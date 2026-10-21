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
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::permission_pattern::{PatternKind, PatternSpec};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const LEGACY_FINGERPRINT_PREFIX: &str = "sha256:v1:";
const ACTION_FINGERPRINT_PREFIX: &str = "hmac-sha256:v2:";
pub type ActionKey = [u8; 32];

/// Describe common Bash operations with fixed words only. This must never copy an
/// argument, path, environment assignment, or arbitrary command into a rule file.
fn safe_bash_description(tool: &str, action: &str) -> Option<&'static str> {
    if !tool.eq_ignore_ascii_case("bash")
        || action.bytes().any(|byte| {
            matches!(
                byte,
                b'\n' | b'\r' | b';' | b'|' | b'&' | b'<' | b'>' | b'$' | b'`'
            )
        })
    {
        return None;
    }
    let mut words = action.split_ascii_whitespace();
    let command = words.next()?;
    let subcommand = words.next();
    let detail = words.next();
    match (command, subcommand, detail) {
        ("npm", Some("run"), Some("lint")) => Some("Run npm lint script"),
        ("npm", Some("run"), Some("typecheck")) => Some("Run npm typecheck script"),
        ("npm", Some("run"), Some("build")) => Some("Run npm build script"),
        ("npm", Some("run"), Some("test:unit")) => Some("Run npm unit tests"),
        ("npm", Some("run"), Some("test:e2e")) => Some("Run npm end-to-end tests"),
        ("npm", Some("run"), _) => Some("Run npm script"),
        ("npm", Some("test"), _) => Some("Run npm tests"),
        ("npm", Some("ci"), _) => Some("Install npm dependencies"),
        ("npm", _, _) => Some("Run npm command"),
        ("cargo", Some("test" | "nextest"), _) => Some("Run Rust tests"),
        ("cargo", Some("check"), _) => Some("Check Rust code"),
        ("cargo", Some("fmt"), _) => Some("Format Rust code"),
        ("cargo", Some("build"), _) => Some("Build Rust code"),
        ("cargo", _, _) => Some("Run Cargo command"),
        ("git", Some("status"), _) => Some("Check Git status"),
        ("git", Some("diff"), _) => Some("Review Git changes"),
        ("git", _, _) => Some("Run Git command"),
        ("rg", _, _) => Some("Search files"),
        ("ls", _, _) => Some("List files"),
        ("date", _, _) => Some("Check date"),
        ("make", _, _) => Some("Run Make target"),
        _ => None,
    }
}

/// Classify non-command tool requests without saving their path, URL, query, or input.
/// The action is used only by Bash's allowlisted command classifier.
fn safe_permission_description(tool: &str, action: &str) -> Option<&'static str> {
    if tool.eq_ignore_ascii_case("bash") {
        return safe_bash_description(tool, action);
    }
    match tool.to_ascii_lowercase().as_str() {
        "read" => Some("Read file"),
        "write" => Some("Write file"),
        "edit" | "multiedit" | "notebookedit" | "filechange" => Some("Edit file"),
        "webfetch" => Some("Fetch web page"),
        "websearch" => Some("Search web"),
        "glob" => Some("Find files"),
        "grep" => Some("Search file contents"),
        _ => None,
    }
}

fn valid_rule_description(tool: &str, description: &str) -> bool {
    if tool.eq_ignore_ascii_case("bash") {
        return valid_bash_description(description);
    }
    safe_permission_description(tool, "") == Some(description)
}

fn valid_bash_description(description: &str) -> bool {
    const ALLOWED: &[&str] = &[
        "Run npm lint script",
        "Run npm typecheck script",
        "Run npm build script",
        "Run npm unit tests",
        "Run npm end-to-end tests",
        "Run npm script",
        "Run npm tests",
        "Install npm dependencies",
        "Run npm command",
        "Run Rust tests",
        "Check Rust code",
        "Format Rust code",
        "Build Rust code",
        "Run Cargo command",
        "Check Git status",
        "Review Git changes",
        "Run Git command",
        "Search files",
        "List files",
        "Check date",
        "Run Make target",
        "Build or test command",
        "Version control command",
        "File operation command",
        "Network command",
        "Other Bash command",
    ];
    ALLOWED.contains(&description)
}

/// Labels a person may choose while reviewing a command. The narrower vocabulary avoids
/// attaching a specific auto-generated operation claim to a different command.
pub fn valid_user_bash_description(description: &str) -> bool {
    matches!(
        description,
        "Build or test command"
            | "Version control command"
            | "File operation command"
            | "Network command"
            | "Other Bash command"
    )
}

fn fresh_action_key() -> io::Result<ActionKey> {
    let mut key = [0; 32];
    getrandom::fill(&mut key).map_err(io::Error::other)?;
    Ok(key)
}

fn key_id(key: &ActionKey) -> String {
    format!("{:x}", Sha256::digest(key))[..16].to_owned()
}

/// HMAC-SHA-256, with a local 256-bit secret absent from exported rule JSON.
fn action_fingerprint(key: &ActionKey, action: &str) -> String {
    const BLOCK: usize = 64;
    let mut inner = [0x36; BLOCK];
    let mut outer = [0x5c; BLOCK];
    for (index, byte) in key.iter().enumerate() {
        inner[index] ^= byte;
        outer[index] ^= byte;
    }
    let mut digest = Sha256::new();
    digest.update(inner);
    digest.update(action.as_bytes());
    let inner_digest = digest.finalize();
    let mut digest = Sha256::new();
    digest.update(outer);
    digest.update(inner_digest);
    format!(
        "{ACTION_FINGERPRINT_PREFIX}{}:{:x}",
        key_id(key),
        digest.finalize()
    )
}

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_legacy_fingerprint(action: &str) -> bool {
    action
        .strip_prefix(LEGACY_FINGERPRINT_PREFIX)
        .is_some_and(|digest| is_hex(digest, 64))
}

fn is_action_fingerprint(action: &str, key: &ActionKey) -> bool {
    action
        .strip_prefix(ACTION_FINGERPRINT_PREFIX)
        .and_then(|rest| rest.split_once(':'))
        .is_some_and(|(id, digest)| id == key_id(key) && is_hex(digest, 64))
}

/// Raw legacy entries can be migrated. A v1 digest or a v2 digest from a lost key
/// cannot be recovered safely and must be discarded rather than auto-approved.
fn stored_action_key(action: &str, key: &ActionKey) -> Option<String> {
    if is_action_fingerprint(action, key) {
        Some(action.to_owned())
    } else if is_legacy_fingerprint(action) || action.starts_with(ACTION_FINGERPRINT_PREFIX) {
        None
    } else {
        Some(action_fingerprint(key, action))
    }
}

/// Obtain the machine-local rule key from the OS credential store. A missing or
/// unreadable key for an existing encrypted file fails closed.
pub fn action_key_for(rules_path: &Path) -> io::Result<ActionKey> {
    crate::permission_vault::key_for(rules_path)
}

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

/// A remembered allow-rule: auto-answer any request matching the exact original action.
/// New rules hold only its fingerprint; raw actions occur only in legacy caller fixtures.
#[derive(Clone, PartialEq, Eq)]
pub struct Rule {
    pub project: String,
    pub tool: String,
    pub action: String,
    /// Original exact action, retained only in memory and inside authenticated
    /// encrypted storage. Never include this in logs or an unauthenticated event.
    pub exact_action: Option<String>,
    /// An explicitly reviewed whole-action matcher. `None` is exact-only.
    pub pattern: Option<PatternSpec>,
    /// A description from a fixed vocabulary. Never contains command arguments or paths.
    pub description: Option<String>,
    pub decision: Decision,
    /// `true` for an `Always` rule the caller should persist; `false` for a `Session`
    /// or `Chat` rule that lives only in memory.
    pub persist: bool,
    /// `Some(connection)` for a chat rule (HS2-JNHAYB) that answers only that connection's
    /// requests; `None` for a project-wide rule.
    pub connection: Option<String>,
}

impl std::fmt::Debug for Rule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rule")
            .field("project", &"[redacted]")
            .field("tool", &self.tool)
            .field("action", &"[redacted]")
            .field(
                "exact_action",
                &self.exact_action.as_ref().map(|_| "[redacted]"),
            )
            .field("pattern", &self.pattern)
            .field("description", &self.description)
            .field("decision", &self.decision)
            .field("persist", &self.persist)
            .field("connection", &self.connection)
            .finish()
    }
}

impl Rule {
    fn matches(&self, project: &str, tool: &str, action: &str, key: &ActionKey) -> bool {
        self.project == project
            && self.tool == tool
            && if let Some(pattern) = &self.pattern {
                pattern.matches(action)
            } else if is_action_fingerprint(&self.action, key) {
                self.action == action_fingerprint(key, action)
            } else if self.action.starts_with(ACTION_FINGERPRINT_PREFIX)
                || is_legacy_fingerprint(&self.action)
            {
                false
            } else {
                self.action == action
            }
    }

    /// Whether two rules occupy the same slot: a newer one replaces an older one.
    fn same_key(&self, other: &Rule) -> bool {
        self.project == other.project
            && self.tool == other.tool
            && self.action == other.action
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
    key: &ActionKey,
) -> Option<&'a Rule> {
    for chat in [true, false] {
        for exact in [true, false] {
            if let Some(rule) = rules.iter().rev().find(|rule| {
                (rule.connection.as_deref() == Some(connection) && chat
                    || rule.connection.is_none() && !chat)
                    && rule.pattern.is_none() == exact
                    && rule.matches(project, tool, action, key)
            }) {
                return Some(rule);
            }
        }
    }
    None
}

/// A remembered rule as a client lists it (HS2-JNHAYB): which scope holds it and what it
/// answers. `scope` is `chat`, `session`, or `always`.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct RuleInfo {
    pub project: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    pub tool: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_action: Option<String>,
    pub match_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<PatternSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub decision: Decision,
    pub scope: Scope,
}

impl std::fmt::Debug for RuleInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuleInfo")
            .field("project", &"[redacted]")
            .field("tool", &self.tool)
            .field("action", &"[redacted]")
            .field(
                "exact_action",
                &self.exact_action.as_ref().map(|_| "[redacted]"),
            )
            .field("match_type", &self.match_type)
            .field("pattern", &self.pattern)
            .field("decision", &self.decision)
            .field("scope", &self.scope)
            .finish()
    }
}

impl From<&Rule> for RuleInfo {
    fn from(rule: &Rule) -> Self {
        Self {
            project: rule.project.clone(),
            connection: rule.connection.clone(),
            tool: rule.tool.clone(),
            action: rule.action.clone(),
            exact_action: rule.exact_action.clone(),
            match_type: match rule.pattern.as_ref().map(|pattern| pattern.kind) {
                Some(PatternKind::Template) => "template",
                Some(PatternKind::Regex) => "regex",
                None => "exact",
            },
            pattern: rule.pattern.clone(),
            description: rule.description.clone(),
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
#[derive(Debug, Clone)]
pub struct PermissionBridge {
    next_id: u64,
    queue: VecDeque<Request>,
    rules: Vec<Rule>,
    action_key: ActionKey,
}

impl Default for PermissionBridge {
    fn default() -> Self {
        Self {
            next_id: 0,
            queue: VecDeque::new(),
            rules: Vec::new(),
            action_key: fresh_action_key().expect("secure randomness for permission rules"),
        }
    }
}

impl PermissionBridge {
    /// A fresh bridge with no rules.
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed durable allow-rules loaded from storage (so a remembered "always" answer
    /// auto-resolves on the next run without re-asking).
    pub fn with_rules(rules: Vec<Rule>) -> Self {
        let mut bridge = Self::new();
        bridge.set_rules(rules);
        bridge
    }

    /// Install a local persistent key and the rules loaded with it. Pending asks stay
    /// pending; old in-memory rules were created under another key and are cleared.
    pub fn set_action_key_and_rules(&mut self, key: ActionKey, rules: Vec<Rule>) {
        self.action_key = key;
        self.rules.clear();
        self.set_rules(rules);
    }

    /// Replace only durable rules after a vault recovers. Session and chat answers
    /// remain in effect, and their opaque action ids are derived from the restored key.
    pub fn restore_durable_rules(&mut self, key: ActionKey, mut rules: Vec<Rule>) {
        let live_rules = self
            .rules
            .drain(..)
            .filter(|rule| !rule.persist)
            .map(|mut rule| {
                if let Some(exact_action) = &rule.exact_action {
                    rule.action = exact_action.clone();
                }
                rule
            })
            .collect::<Vec<_>>();
        rules.extend(live_rules);
        self.action_key = key;
        self.set_rules(rules);
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
        if let Some(rule) = effective_rule(
            &self.rules,
            &project,
            &connection,
            &tool,
            &action,
            &self.action_key,
        ) {
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
        self.rules = rules
            .into_iter()
            .filter_map(|mut rule| {
                if rule.exact_action.is_none()
                    && !is_action_fingerprint(&rule.action, &self.action_key)
                {
                    rule.exact_action = Some(rule.action.clone());
                }
                // A fingerprint without its authenticated plaintext cannot be reviewed.
                rule.exact_action.as_ref()?;
                rule.action = stored_action_key(&rule.action, &self.action_key)?;
                rule.description = rule
                    .description
                    .filter(|value| valid_rule_description(&rule.tool, value));
                Some(rule)
            })
            .collect();
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
            rule.project == project
                && rule.tool == tool
                && (rule.matches(project, tool, action, &self.action_key) || rule.action == action)
                && rule.connection.as_deref() == connection
        })?;
        Some(self.rules.remove(position))
    }

    /// Change the answer in an existing exact slot without changing its lifetime or order.
    pub fn update_decision(
        &mut self,
        project: &str,
        connection: Option<&str>,
        tool: &str,
        action: &str,
        decision: Decision,
    ) -> Option<Rule> {
        let rule = self.rules.iter_mut().find(|rule| {
            rule.project == project
                && rule.tool == tool
                && rule.action == action
                && rule.connection.as_deref() == connection
        })?;
        rule.decision = decision;
        Some(rule.clone())
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
        self.resolve_with_description(id, decision, scope, None)
    }

    /// Resolve a request with an optional label chosen from the fixed safe Bash vocabulary.
    /// The label is accepted only for Bash; the original action is never copied into a rule.
    pub fn resolve_with_description(
        &mut self,
        id: u64,
        decision: Decision,
        scope: Scope,
        description: Option<&str>,
    ) -> Option<Resolved> {
        self.resolve_with_pattern(id, decision, scope, description, None)
            .expect("exact permission rule is valid")
    }

    pub fn resolve_with_pattern(
        &mut self,
        id: u64,
        decision: Decision,
        scope: Scope,
        description: Option<&str>,
        pattern: Option<PatternSpec>,
    ) -> io::Result<Option<Resolved>> {
        if let Some(pattern) = &pattern {
            if scope == Scope::Once {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "one-time answers cannot contain a pattern",
                ));
            }
            let Some(request) = self.queue.iter().find(|request| request.id == id) else {
                return Ok(None);
            };
            pattern.validate(&request.action)?;
        }
        let Some(pos) = self.queue.iter().position(|r| r.id == id) else {
            return Ok(None);
        };
        let req = self.queue.remove(pos).expect("position just found");

        let (persisted_rule, cascaded) = if scope == Scope::Once {
            (None, Vec::new())
        } else {
            let rule = self.rule_for_request(&req, decision, scope, description, pattern);
            let persisted = (scope == Scope::Always).then(|| rule.clone());
            (persisted, self.install_rule(rule))
        };

        Ok(Some(Resolved {
            project: req.project,
            connection: req.connection,
            decision,
            persisted_rule,
            cascaded,
        }))
    }

    fn rule_for_request(
        &self,
        request: &Request,
        decision: Decision,
        scope: Scope,
        description: Option<&str>,
        pattern: Option<PatternSpec>,
    ) -> Rule {
        Rule {
            project: request.project.clone(),
            tool: request.tool.clone(),
            action: action_fingerprint(
                &self.action_key,
                &pattern
                    .as_ref()
                    .map_or_else(|| request.action.clone(), PatternSpec::identity),
            ),
            exact_action: Some(request.action.clone()),
            pattern,
            description: description
                .filter(|value| {
                    request.tool.eq_ignore_ascii_case("bash") && valid_user_bash_description(value)
                })
                .or_else(|| safe_permission_description(&request.tool, &request.action))
                .map(str::to_owned),
            decision,
            persist: scope == Scope::Always,
            connection: (scope == Scope::Chat).then(|| request.connection.clone()),
        }
    }

    fn install_rule(&mut self, rule: Rule) -> Vec<CascadedResolution> {
        self.rules.retain(|existing| !existing.same_key(&rule));
        self.rules.push(rule);
        let rules = &self.rules;
        let (answered, waiting): (Vec<Request>, Vec<Request>) =
            self.queue.drain(..).partition(|queued| {
                effective_rule(
                    rules,
                    &queued.project,
                    &queued.connection,
                    &queued.tool,
                    &queued.action,
                    &self.action_key,
                )
                .is_some()
            });
        self.queue = waiting.into();
        answered
            .into_iter()
            .map(|queued| {
                let decision = effective_rule(
                    &self.rules,
                    &queued.project,
                    &queued.connection,
                    &queued.tool,
                    &queued.action,
                    &self.action_key,
                )
                .expect("partitioned by a matching rule")
                .decision;
                CascadedResolution {
                    id: queued.id,
                    project: queued.project,
                    connection: queued.connection,
                    decision,
                }
            })
            .collect()
    }

    /// Install a reviewed rule after its original request was already allowed once.
    /// Pending requests that match the new rule are resolved normally.
    pub fn remember_allowed_request(
        &mut self,
        request: &Request,
        description: Option<&str>,
        pattern: Option<PatternSpec>,
    ) -> io::Result<(Rule, Vec<CascadedResolution>)> {
        if let Some(pattern) = &pattern {
            pattern.validate(&request.action)?;
        }
        let rule = self.rule_for_request(
            request,
            Decision::Allow,
            Scope::Always,
            description,
            pattern,
        );
        let cascaded = self.install_rule(rule.clone());
        Ok((rule, cascaded))
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
    durable_validator: Mutex<Option<Box<dyn Fn() -> bool + Send + Sync>>>,
    durable_locked: AtomicBool,
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
            durable_validator: Mutex::new(None),
            durable_locked: AtomicBool::new(false),
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

    /// Recheck the host key and encrypted rule data before any durable auto-answer.
    /// On failure, old in-memory approvals are withdrawn and the request asks again.
    pub fn set_durable_validator(&self, f: impl Fn() -> bool + Send + Sync + 'static) {
        *self.durable_validator.lock_or_recover() = Some(Box::new(f));
    }

    fn enforce_durable_health(&self, bridge: &mut PermissionBridge) {
        if !bridge.rules.iter().any(|rule| rule.persist) {
            return;
        }
        if self
            .durable_validator
            .lock_or_recover()
            .as_ref()
            .is_some_and(|check| !check())
        {
            self.durable_locked.store(true, Ordering::Release);
            bridge.rules.retain(|rule| !rule.persist);
        }
    }

    pub fn drop_durable_rules(&self) {
        self.durable_locked.store(true, Ordering::Release);
        self.inner
            .lock_or_recover()
            .rules
            .retain(|rule| !rule.persist);
    }

    pub fn durable_rules_locked(&self) -> bool {
        self.durable_locked.load(Ordering::Acquire)
    }

    /// Replace the bridge's allow-rules (e.g. seeding durable `Always` rules loaded from
    /// disk at startup) without disturbing the observer or any in-flight waiters.
    pub fn reseed_rules(&self, rules: Vec<Rule>) {
        self.inner.lock_or_recover().set_rules(rules);
    }

    /// Seed rules under the persistent local key loaded beside their rule file.
    pub fn reseed_rules_with_key(&self, key: ActionKey, rules: Vec<Rule>) {
        self.inner
            .lock_or_recover()
            .set_action_key_and_rules(key, rules);
        self.durable_locked.store(false, Ordering::Release);
    }

    pub fn restore_durable_rules_with_key(&self, key: ActionKey, rules: Vec<Rule>) {
        self.inner
            .lock_or_recover()
            .restore_durable_rules(key, rules);
        self.durable_locked.store(false, Ordering::Release);
    }

    /// Add another project's durable rules without disturbing already seeded projects.
    pub fn add_rules(&self, rules: impl IntoIterator<Item = Rule>) {
        let mut bridge = self.inner.lock_or_recover();
        for mut rule in rules {
            if rule.exact_action.is_none() {
                continue;
            }
            if let Some(action) = stored_action_key(&rule.action, &bridge.action_key) {
                rule.action = action;
                bridge.rules.push(rule);
            }
        }
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
        let outcome = {
            let mut bridge = self.inner.lock_or_recover();
            self.enforce_durable_health(&mut bridge);
            bridge.request_for_project(
                project.clone(),
                connection.clone(),
                tool.clone(),
                action.clone(),
            )
        };
        let id = match outcome {
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
        let outcome = {
            let mut bridge = self.inner.lock_or_recover();
            self.enforce_durable_health(&mut bridge);
            bridge.request_for_project_as(
                project.clone(),
                connection.clone(),
                tool.clone(),
                action.clone(),
                agent.clone(),
            )
        };
        let id = match outcome {
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
        self.resolve_with_description(id, decision, scope, None)
    }

    pub fn resolve_with_description(
        &self,
        id: u64,
        decision: Decision,
        scope: Scope,
        description: Option<&str>,
    ) -> Option<Resolved> {
        self.resolve_with_reviewed_pattern(id, decision, scope, description, None)
            .expect("exact permission rule is valid")
    }

    pub fn resolve_with_reviewed_pattern(
        &self,
        id: u64,
        decision: Decision,
        scope: Scope,
        description: Option<&str>,
        pattern: Option<PatternSpec>,
    ) -> io::Result<Option<Resolved>> {
        let Some(resolved) = self.inner.lock_or_recover().resolve_with_pattern(
            id,
            decision,
            scope,
            description,
            pattern,
        )?
        else {
            return Ok(None);
        };
        {
            let mut results = self.results.lock_or_recover();
            results.insert(id, decision);
            // Identical waiting asks the new rule answered wake with its decision too.
            for cascaded in &resolved.cascaded {
                results.insert(cascaded.id, cascaded.decision);
            }
        }
        self.wake_waiters();
        Ok(Some(resolved))
    }

    /// Persist an Always answer before installing it or waking any queued request.
    /// An unavailable key or failed encrypted write leaves the original request pending.
    pub fn resolve_persisted(
        &self,
        id: u64,
        decision: Decision,
        description: Option<&str>,
        path: &Path,
    ) -> io::Result<Option<Resolved>> {
        self.resolve_persisted_with_pattern(id, decision, description, None, path)
    }

    pub fn resolve_persisted_with_pattern(
        &self,
        id: u64,
        decision: Decision,
        description: Option<&str>,
        pattern: Option<PatternSpec>,
        path: &Path,
    ) -> io::Result<Option<Resolved>> {
        let mut bridge = self.inner.lock_or_recover();
        let mut proposed = bridge.clone();
        let Some(resolved) =
            proposed.resolve_with_pattern(id, decision, Scope::Always, description, pattern)?
        else {
            return Ok(None);
        };
        let rule = resolved
            .persisted_rule
            .as_ref()
            .expect("Always creates a durable rule");
        append_rule(path, rule)?;
        *bridge = proposed;
        drop(bridge);
        {
            let mut results = self.results.lock_or_recover();
            results.insert(id, decision);
            for cascaded in &resolved.cascaded {
                results.insert(cascaded.id, cascaded.decision);
            }
        }
        self.wake_waiters();
        Ok(Some(resolved))
    }

    /// Persist a reviewed Always rule after the original request was allowed once.
    /// A failed write leaves both live rules and other pending asks unchanged.
    pub fn remember_allowed_request(
        &self,
        request: &Request,
        description: Option<&str>,
        pattern: Option<PatternSpec>,
        path: &Path,
    ) -> io::Result<Vec<CascadedResolution>> {
        let mut bridge = self.inner.lock_or_recover();
        let mut proposed = bridge.clone();
        let (rule, cascaded) = proposed.remember_allowed_request(request, description, pattern)?;
        append_rule(path, &rule)?;
        *bridge = proposed;
        drop(bridge);
        if !cascaded.is_empty() {
            let mut results = self.results.lock_or_recover();
            for answered in &cascaded {
                results.insert(answered.id, answered.decision);
            }
            drop(results);
            self.wake_waiters();
        }
        Ok(cascaded)
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

    /// Persist a durable revocation before removing the live answer. A failed vault write
    /// must leave the remembered rule intact so Settings can report and retry it.
    pub fn revoke_with_storage(
        &self,
        project: &str,
        connection: Option<&str>,
        tool: &str,
        action: &str,
        durable_path: Option<&Path>,
    ) -> io::Result<Option<Rule>> {
        let mut bridge = self.inner.lock_or_recover();
        let Some(current) = bridge.rules.iter().find(|rule| {
            rule.project == project
                && rule.connection.as_deref() == connection
                && rule.tool == tool
                && (rule.action == action
                    || rule.matches(project, tool, action, &bridge.action_key))
        }) else {
            return Ok(None);
        };
        if current.persist {
            let path = durable_path
                .ok_or_else(|| io::Error::other("permission rule store unavailable"))?;
            if !remove_rule(path, project, tool, action)? {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "durable permission rule missing",
                ));
            }
        }
        let removed = bridge.revoke(project, connection, tool, action);
        if removed.as_ref().is_some_and(|rule| rule.persist) {
            bridge.revoke("", None, tool, action);
        }
        Ok(removed)
    }

    /// Change an exact remembered answer while holding the bridge lock across durable
    /// persistence. A file error leaves the live rule unchanged.
    pub fn update_decision(
        &self,
        project: &str,
        connection: Option<&str>,
        tool: &str,
        action: &str,
        decision: Decision,
        durable_path: Option<&Path>,
    ) -> io::Result<Option<Rule>> {
        let mut bridge = self.inner.lock_or_recover();
        let Some(current) = bridge.rules().iter().find(|rule| {
            rule.project == project
                && rule.connection.as_deref() == connection
                && rule.tool == tool
                && rule.action == action
        }) else {
            return Ok(None);
        };
        let persist = current.persist;
        if persist {
            let path = durable_path
                .ok_or_else(|| io::Error::other("permission rule store unavailable"))?;
            update_stored_rule_decision(path, project, tool, action, decision)?;
        }
        let updated = bridge.update_decision(project, connection, tool, action, decision);
        if persist {
            bridge.update_decision("", None, tool, action, decision);
        }
        Ok(updated)
    }

    /// Replace a reviewed matcher without changing the decision or lifetime. The
    /// encrypted file is written before the live rule changes.
    pub fn update_pattern(
        &self,
        project: &str,
        connection: Option<&str>,
        tool: &str,
        action: &str,
        pattern: Option<PatternSpec>,
        durable_path: Option<&Path>,
    ) -> io::Result<Option<Rule>> {
        let mut bridge = self.inner.lock_or_recover();
        let Some(index) = bridge.rules.iter().position(|rule| {
            rule.project == project
                && rule.connection.as_deref() == connection
                && rule.tool == tool
                && rule.action == action
        }) else {
            return Ok(None);
        };
        let current = bridge.rules[index].clone();
        let original = current.exact_action.as_deref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "remembered action unavailable")
        })?;
        if let Some(spec) = &pattern {
            spec.validate(original)?;
        }
        let key_material = pattern
            .as_ref()
            .map_or_else(|| original.to_owned(), PatternSpec::identity);
        let next_action = action_fingerprint(&bridge.action_key, &key_material);
        if bridge.rules.iter().enumerate().any(|(position, rule)| {
            position != index
                && rule.project == project
                && rule.connection.as_deref() == connection
                && rule.tool == tool
                && rule.action == next_action
        }) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "matching rule already exists",
            ));
        }
        if current.persist {
            let path = durable_path
                .ok_or_else(|| io::Error::other("permission rule store unavailable"))?;
            crate::permission_vault::mutate(path, |rules| {
                let stored = rules
                    .iter_mut()
                    .find(|rule| {
                        rule.project == project
                            && rule.tool == tool
                            && action_fingerprint(&bridge.action_key, &rule.key_material())
                                == action
                    })
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::NotFound, "durable permission rule missing")
                    })?;
                stored.pattern = pattern.clone();
                Ok(())
            })?;
        }
        bridge.rules[index].pattern = pattern.clone();
        bridge.rules[index].action = next_action.clone();
        if current.persist {
            for rule in &mut bridge.rules {
                if rule.project.is_empty()
                    && rule.connection.is_none()
                    && rule.tool == tool
                    && rule.action == action
                {
                    rule.pattern = pattern.clone();
                    rule.action = next_action.clone();
                }
            }
        }
        Ok(Some(bridge.rules[index].clone()))
    }

    /// Change a project-wide rule between server-session and durable lifetimes. Write the
    /// durable file first; a failed write leaves the live answer and its lifetime intact.
    /// The primary store has an empty-project mirror for legacy requests (HS2-V95VEB).
    pub fn update_scope(
        &self,
        project: &str,
        connection: Option<&str>,
        tool: &str,
        action: &str,
        persist: bool,
        durable: Option<(&Path, bool)>,
    ) -> io::Result<Option<Rule>> {
        let mut bridge = self.inner.lock_or_recover();
        let Some(index) = bridge.rules.iter().position(|rule| {
            rule.project == project
                && rule.connection.as_deref() == connection
                && rule.tool == tool
                && rule.action == action
        }) else {
            return Ok(None);
        };
        let current = bridge.rules[index].clone();
        if current.connection.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "chat-scoped permission rules cannot change lifetime",
            ));
        }
        if current.persist == persist {
            return Ok(Some(current));
        }
        let (path, mirror_legacy) =
            durable.ok_or_else(|| io::Error::other("permission rule store unavailable"))?;
        if persist {
            append_rule(
                path,
                &Rule {
                    persist: true,
                    ..current.clone()
                },
            )?;
        } else if !remove_rule(path, project, tool, action)? {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "durable permission rule missing",
            ));
        }
        bridge.rules[index].persist = persist;
        let updated = bridge.rules[index].clone();
        if mirror_legacy {
            bridge.rules.retain(|rule| {
                !(rule.project.is_empty()
                    && rule.connection.is_none()
                    && rule.tool == tool
                    && rule.action == action)
            });
            if persist {
                bridge.rules.push(Rule {
                    project: String::new(),
                    ..updated.clone()
                });
            }
        }
        Ok(Some(updated))
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

/// The plaintext form inside the authenticated encrypted v3 envelope. This type must
/// never be serialized directly to the rule path, an event, or a log.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRule {
    #[serde(default)]
    pub project: String,
    pub tool: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<PatternSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub decision: Decision,
}

impl std::fmt::Debug for StoredRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredRule")
            .field("project", &"[redacted]")
            .field("tool", &self.tool)
            .field("action", &"[redacted]")
            .field("pattern", &self.pattern)
            .field("description", &"[redacted]")
            .field("decision", &self.decision)
            .finish()
    }
}

impl From<&Rule> for StoredRule {
    fn from(r: &Rule) -> Self {
        StoredRule {
            project: r.project.clone(),
            tool: r.tool.clone(),
            action: r.exact_action.clone().unwrap_or_else(|| r.action.clone()),
            pattern: r.pattern.clone(),
            description: r.description.clone(),
            decision: r.decision,
        }
    }
}

impl StoredRule {
    pub(crate) fn key_material(&self) -> String {
        self.pattern
            .as_ref()
            .map_or_else(|| self.action.clone(), PatternSpec::identity)
    }

    /// Rehydrate a durable (persisted) [`Rule`] for seeding a bridge.
    pub fn into_rule(self, key: &ActionKey) -> Rule {
        let key_material = self.key_material();
        Rule {
            project: self.project,
            tool: self.tool,
            action: action_fingerprint(key, &key_material),
            exact_action: Some(self.action),
            pattern: self.pattern,
            description: self.description,
            decision: self.decision,
            persist: true,
            connection: None,
        }
    }
}

/// Load authenticated encrypted durable rules. Legacy fingerprint arrays are discarded
/// on upgrade, and an unavailable key or invalid envelope activates no rules.
pub fn load_rules(path: &Path) -> Vec<Rule> {
    load_rules_with_key(path)
        .map(|(_, rules)| rules)
        .unwrap_or_default()
}

/// Read and authenticate the vault and its action key under one file lock. Callers
/// recovering a locked bridge must see failures instead of treating them as an empty vault.
pub fn load_rules_with_key(path: &Path) -> io::Result<(ActionKey, Vec<Rule>)> {
    let (key, stored) = crate::permission_vault::load_with_key(path)?;
    let rules = stored
        .into_iter()
        .filter_map(|mut rule| {
            rule.description = rule
                .description
                .filter(|value| valid_rule_description(&rule.tool, value));
            if rule
                .pattern
                .as_ref()
                .is_some_and(|pattern| pattern.validate(&rule.action).is_err())
            {
                return None;
            }
            Some(rule.into_rule(&key))
        })
        .collect();
    Ok((key, rules))
}

/// Check key availability and authenticate the on-disk envelope for a local server.
pub fn rules_health(path: &Path) -> io::Result<()> {
    crate::permission_vault::load(path).map(|_| ())
}

/// Remove a durable rule for `(project, tool, action)` from `path` (HS2-JNHAYB revoke).
/// Returns whether one was removed; a missing file removes nothing.
pub fn remove_rule(path: &Path, project: &str, tool: &str, action: &str) -> std::io::Result<bool> {
    let key = action_key_for(path)?;
    crate::permission_vault::mutate(path, |rules| {
        let before = rules.len();
        rules.retain(|r| {
            !((r.project == project || r.project.is_empty())
                && r.tool == tool
                && (r.action == action || action_fingerprint(&key, &r.key_material()) == action))
        });
        Ok(rules.len() != before)
    })
}

/// Edit a durable answer in place. Preserve its file order so an edit does not change
/// the newest-first Settings list after a restart.
fn update_stored_rule_decision(
    path: &Path,
    project: &str,
    tool: &str,
    action: &str,
    decision: Decision,
) -> io::Result<()> {
    let key = action_key_for(path)?;
    crate::permission_vault::mutate(path, |rules| {
        let mut found = false;
        for rule in rules {
            if (rule.project == project || rule.project.is_empty())
                && rule.tool == tool
                && (rule.action == action
                    || action_fingerprint(&key, &rule.key_material()) == action)
            {
                rule.decision = decision;
                found = true;
            }
        }
        if found {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "durable permission rule missing",
            ))
        }
    })
}

/// Persist an `Always` allow-rule to `path`, replacing any existing rule for the same
/// `(project, tool, action)` (a later answer wins) — so remembered answers survive a restart.
pub fn append_rule(path: &Path, rule: &Rule) -> std::io::Result<()> {
    let action = rule.exact_action.as_deref().unwrap_or(&rule.action);
    if rule.exact_action.is_none()
        && (action.starts_with(ACTION_FINGERPRINT_PREFIX) || is_legacy_fingerprint(action))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "permission rule lacks exact action",
        ));
    }
    let mut stored = StoredRule::from(rule);
    stored.action = action.into();
    if let Some(pattern) = &stored.pattern {
        pattern.validate(&stored.action)?;
    }
    stored.description = stored
        .description
        .filter(|value| valid_rule_description(&stored.tool, value));
    crate::permission_vault::mutate(path, |rules| {
        rules.retain(|r| {
            !(r.project == stored.project
                && r.tool == stored.tool
                && r.key_material() == stored.key_material())
        });
        rules.push(stored);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reviewed_pattern_is_scoped_and_exact_rule_takes_precedence() {
        let mut bridge = PermissionBridge::new();
        let exact = match bridge.request_for_project(
            "p",
            "chat-a",
            "Bash",
            "curl -H 'Bearer fixed' https://site.test",
        ) {
            Outcome::Pending(id) => id,
            _ => panic!("new action must ask"),
        };
        bridge.resolve(exact, Decision::Deny, Scope::Chat);
        let first = match bridge.request_for_project(
            "p",
            "chat-a",
            "Bash",
            "curl -H 'Bearer old' https://site.test",
        ) {
            Outcome::Pending(id) => id,
            _ => panic!("new action must ask"),
        };
        let pattern = PatternSpec {
            version: 1,
            kind: PatternKind::Template,
            expression: "curl -H 'Bearer {{token}}' https://site.test".into(),
        };
        assert!(
            bridge
                .resolve_with_pattern(
                    first,
                    Decision::Allow,
                    Scope::Chat,
                    None,
                    Some(PatternSpec {
                        version: 1,
                        kind: PatternKind::Regex,
                        expression: ".*".into()
                    })
                )
                .is_err()
        );
        assert!(bridge.pending().any(|request| request.id == first));
        bridge
            .resolve_with_pattern(first, Decision::Allow, Scope::Chat, None, Some(pattern))
            .unwrap();
        assert_eq!(
            bridge.request_for_project(
                "p",
                "chat-a",
                "Bash",
                "curl -H 'Bearer fixed' https://site.test"
            ),
            Outcome::Auto(Decision::Deny)
        );
        assert_eq!(
            bridge.request_for_project(
                "p",
                "chat-a",
                "Bash",
                "curl -H 'Bearer next' https://site.test"
            ),
            Outcome::Auto(Decision::Allow)
        );
        assert!(matches!(
            bridge.request_for_project(
                "p",
                "chat-b",
                "Bash",
                "curl -H 'Bearer next' https://site.test"
            ),
            Outcome::Pending(_)
        ));
        assert!(matches!(
            bridge.request_for_project(
                "other",
                "chat-a",
                "Bash",
                "curl -H 'Bearer next' https://site.test"
            ),
            Outcome::Pending(_)
        ));
        assert!(matches!(
            bridge.request_for_project(
                "p",
                "chat-a",
                "WebFetch",
                "curl -H 'Bearer next' https://site.test"
            ),
            Outcome::Pending(_)
        ));
        assert!(matches!(
            bridge.request_for_project(
                "p",
                "chat-a",
                "Bash",
                "curl -H 'Bearer next' https://evil.test"
            ),
            Outcome::Pending(_)
        ));
    }

    #[test]
    fn bash_description_uses_only_fixed_vocabulary_and_rejects_compound_commands() {
        assert_eq!(
            safe_bash_description("Bash", "npm run test:unit -- --token=secret"),
            Some("Run npm unit tests")
        );
        assert_eq!(
            safe_bash_description("Bash", "cargo test private::case"),
            Some("Run Rust tests")
        );
        assert_eq!(
            safe_bash_description("Bash", "npm run private-script"),
            Some("Run npm script")
        );
        for action in [
            "API_TOKEN=secret npm test",
            "npm test && echo secret",
            "npm test; cat secret",
            "npm test\ncat secret",
            "npm test $(cat secret)",
            "curl https://example.test/private",
        ] {
            assert_eq!(safe_bash_description("Bash", action), None, "{action}");
        }
        assert_eq!(safe_bash_description("WebFetch", "npm test"), None);
    }

    #[test]
    fn non_command_descriptions_use_fixed_words_and_survive_a_durable_reload() {
        let secret = "https://private.example/path?token=secret";
        assert_eq!(
            safe_permission_description("WebFetch", secret),
            Some("Fetch web page")
        );
        assert_eq!(
            safe_permission_description("Read", "/private/customer.txt"),
            Some("Read file")
        );
        assert_eq!(safe_permission_description("UnknownTool", secret), None);
        assert!(!valid_rule_description("WebFetch", secret));
        assert!(!valid_rule_description("Read", "Fetch web page"));

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        let mut bridge = PermissionBridge::new();
        bridge.set_action_key_and_rules(action_key_for(&path).unwrap(), Vec::new());
        let id = match bridge.request_for_project("p", "chat", "WebFetch", secret) {
            Outcome::Pending(id) => id,
            _ => panic!("new action must ask"),
        };
        let rule = bridge
            .resolve(id, Decision::Allow, Scope::Always)
            .unwrap()
            .persisted_rule
            .unwrap();
        assert_eq!(rule.description.as_deref(), Some("Fetch web page"));
        append_rule(&path, &rule).unwrap();
        assert_eq!(
            load_rules(&path)[0].description.as_deref(),
            Some("Fetch web page")
        );
        assert!(!std::fs::read_to_string(&path).unwrap().contains(secret));

        crate::permission_vault::mutate(&path, |rules| {
            rules[0].description = Some(secret.into());
            Ok(())
        })
        .unwrap();
        assert_eq!(load_rules(&path)[0].description, None);
    }

    #[test]
    fn bash_descriptions_follow_out_of_order_rules_and_scrub_untrusted_storage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        let mut bridge = PermissionBridge::new();
        bridge.set_action_key_and_rules(action_key_for(&path).unwrap(), Vec::new());
        let known = match bridge.request_for_project("p", "chat", "Bash", "npm run test:unit") {
            Outcome::Pending(id) => id,
            _ => panic!("known operation must ask"),
        };
        let unknown = match bridge.request_for_project("p", "chat", "Bash", "curl private-url") {
            Outcome::Pending(id) => id,
            _ => panic!("unknown operation must ask"),
        };
        bridge
            .resolve(unknown, Decision::Deny, Scope::Session)
            .unwrap();
        let saved = bridge
            .resolve(known, Decision::Allow, Scope::Always)
            .unwrap()
            .persisted_rule
            .unwrap();
        assert_eq!(bridge.rules()[0].description, None);
        assert_eq!(
            bridge.rules()[1].description.as_deref(),
            Some("Run npm unit tests")
        );
        assert_eq!(saved.description.as_deref(), Some("Run npm unit tests"));

        append_rule(&path, &saved).unwrap();
        assert_eq!(
            load_rules(&path)[0].description.as_deref(),
            Some("Run npm unit tests")
        );

        crate::permission_vault::mutate(&path, |rules| {
            rules[0].description = Some("private command text".into());
            Ok(())
        })
        .unwrap();
        assert_eq!(load_rules(&path)[0].description, None);
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("private command text")
        );

        let mut untrusted = saved;
        untrusted.description = Some("private command text".into());
        append_rule(&path, &untrusted).unwrap();
        assert_eq!(load_rules(&path)[0].description, None);
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("private command text")
        );
    }

    #[test]
    fn chosen_bash_labels_are_safe_and_follow_reapproval_without_command_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        let mut bridge = PermissionBridge::new();
        bridge.set_action_key_and_rules(action_key_for(&path).unwrap(), Vec::new());
        let action = "curl https://private.example/token=secret";
        let first = match bridge.request_for_project("p", "chat", "Bash", action) {
            Outcome::Pending(id) => id,
            _ => panic!("unknown action must ask"),
        };
        let initial = bridge
            .resolve(first, Decision::Allow, Scope::Always)
            .unwrap()
            .persisted_rule
            .unwrap();
        append_rule(&path, &initial).unwrap();
        assert_eq!(initial.description, None);
        assert!(bridge.revoke("p", None, "Bash", &initial.action).is_some());
        assert!(remove_rule(&path, "p", "Bash", &initial.action).unwrap());

        let next = match bridge.request_for_project("p", "chat", "Bash", action) {
            Outcome::Pending(id) => id,
            _ => panic!("revoked action must ask again"),
        };
        let approved = bridge
            .resolve_with_description(
                next,
                Decision::Allow,
                Scope::Always,
                Some("Network command"),
            )
            .unwrap()
            .persisted_rule
            .unwrap();
        append_rule(&path, &approved).unwrap();
        assert_eq!(approved.description.as_deref(), Some("Network command"));
        assert_eq!(
            bridge.request_for_project("p", "chat", "Bash", action),
            Outcome::Auto(Decision::Allow)
        );
        let durable = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            load_rules(&path)[0].description.as_deref(),
            Some("Network command")
        );
        assert!(!durable.contains("Network command"));
        assert!(!durable.contains("private.example"));
        assert!(!durable.contains("secret"));

        let other = match bridge.request_for_project("p", "chat", "Bash", "private command") {
            Outcome::Pending(id) => id,
            _ => panic!("other action must ask"),
        };
        bridge.resolve_with_description(
            other,
            Decision::Allow,
            Scope::Session,
            Some("secret text"),
        );
        assert_eq!(bridge.rules().last().unwrap().description, None);
    }

    #[test]
    fn session_and_always_edits_preserve_answer_and_mirror_only_after_durable_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");
        let key = action_key_for(&path).unwrap();
        let action = action_fingerprint(&key, "private command");
        let rule = Rule {
            project: "p".into(),
            tool: "Bash".into(),
            action: action.clone(),
            exact_action: Some("private command".into()),
            pattern: None,
            description: None,
            decision: Decision::Deny,
            persist: false,
            connection: None,
        };
        let bridge = SharedPermissionBridge::new(PermissionBridge::new());
        bridge.reseed_rules_with_key(key, vec![rule.clone()]);
        assert!(
            bridge
                .update_scope("p", None, "Bash", &action, true, None)
                .is_err()
        );
        assert_eq!(bridge.rules()[0].scope, Scope::Session);
        assert!(
            bridge
                .update_scope("p", None, "Bash", "other", true, Some((&path, true)))
                .unwrap()
                .is_none()
        );

        let promoted = bridge
            .update_scope("p", None, "Bash", &action, true, Some((&path, true)))
            .unwrap()
            .unwrap();
        assert_eq!(promoted.decision, Decision::Deny);
        assert!(promoted.persist);
        assert_eq!(load_rules(&path), vec![promoted.clone()]);
        assert_eq!(
            bridge
                .rules()
                .iter()
                .filter(|rule| rule.scope == Scope::Always)
                .count(),
            2
        );
        assert_eq!(
            bridge.inner.lock_or_recover().request_for_project(
                "",
                "legacy",
                "Bash",
                "private command"
            ),
            Outcome::Auto(Decision::Deny)
        );
        let restarted = SharedPermissionBridge::new(PermissionBridge::new());
        restarted.reseed_rules_with_key(key, load_rules(&path));
        assert_eq!(restarted.rules()[0].scope, Scope::Always);

        bridge
            .update_scope("p", None, "Bash", &action, false, Some((&path, true)))
            .unwrap();
        assert!(load_rules(&path).is_empty());
        assert_eq!(bridge.rules().len(), 1);
        assert_eq!(bridge.rules()[0].scope, Scope::Session);
        restarted.reseed_rules_with_key(key, load_rules(&path));
        assert!(restarted.rules().is_empty());

        bridge.add_rules([Rule {
            connection: Some("chat".into()),
            ..rule
        }]);
        assert_eq!(
            bridge
                .update_scope(
                    "p",
                    Some("chat"),
                    "Bash",
                    &action,
                    true,
                    Some((&path, true)),
                )
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(load_rules(&path).is_empty());
    }

    #[test]
    fn reviewed_pattern_edit_updates_encrypted_rule_and_restart_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");
        let key = action_key_for(&path).unwrap();
        let original = "curl -H 'Bearer old' https://site.test";
        let action = action_fingerprint(&key, original);
        let rule = Rule {
            project: "p".into(),
            tool: "Bash".into(),
            action: action.clone(),
            exact_action: Some(original.into()),
            pattern: None,
            description: None,
            decision: Decision::Allow,
            persist: true,
            connection: None,
        };
        append_rule(&path, &rule).unwrap();
        let bridge = SharedPermissionBridge::new(PermissionBridge::new());
        bridge.reseed_rules_with_key(key, load_rules(&path));
        let pattern = PatternSpec {
            version: 1,
            kind: PatternKind::Template,
            expression: "curl -H 'Bearer {{token}}' https://site.test".into(),
        };
        let changed = bridge
            .update_pattern(
                "p",
                None,
                "Bash",
                &action,
                Some(pattern.clone()),
                Some(&path),
            )
            .unwrap()
            .unwrap();
        assert_eq!(changed.pattern, Some(pattern));
        assert_ne!(changed.action, action);
        assert!(!fs::read_to_string(&path).unwrap().contains("{{token}}"));
        let restarted = SharedPermissionBridge::new(PermissionBridge::new());
        restarted.reseed_rules_with_key(key, load_rules(&path));
        assert_eq!(
            restarted.inner.lock_or_recover().request_for_project(
                "p",
                "chat",
                "Bash",
                "curl -H 'Bearer next' https://site.test"
            ),
            Outcome::Auto(Decision::Allow)
        );
        let exact = bridge
            .update_pattern("p", None, "Bash", &changed.action, None, Some(&path))
            .unwrap()
            .unwrap();
        assert_eq!(exact.action, action);
        assert_eq!(load_rules(&path)[0].pattern, None);
    }

    #[test]
    fn editing_durable_rule_keeps_storage_order_and_rejects_missing_slots() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");
        for action in ["first", "second"] {
            append_rule(
                &path,
                &Rule {
                    project: "p".into(),
                    tool: "Bash".into(),
                    action: action.into(),
                    exact_action: None,
                    pattern: None,
                    description: None,
                    decision: Decision::Allow,
                    persist: true,
                    connection: None,
                },
            )
            .unwrap();
        }
        let before = load_rules(&path);
        update_stored_rule_decision(&path, "p", "Bash", &before[0].action, Decision::Deny).unwrap();
        let after = load_rules(&path);
        assert_eq!(
            after.iter().map(|rule| &rule.action).collect::<Vec<_>>(),
            before.iter().map(|rule| &rule.action).collect::<Vec<_>>()
        );
        assert_eq!(after[0].decision, Decision::Deny);
        assert_eq!(after[1].decision, Decision::Allow);
        assert_eq!(
            update_stored_rule_decision(&path, "q", "Bash", &before[0].action, Decision::Allow)
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn keyed_fingerprint_matches_an_independent_hmac_sha256_vector() {
        let key = [0x0b; 32];
        assert_eq!(
            action_fingerprint(&key, "Hi There"),
            format!(
                "hmac-sha256:v2:{}:198a607eb44bfbc69903a0f1cf2bbdc5ba0aa3f3d9ae3c1c7a3b1696a0b68cf7",
                key_id(&key)
            )
        );
    }

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
                exact_action: None,
                pattern: None,
                description: None,
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
                exact_action: None,
                pattern: None,
                description: None,
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
                exact_action: None,
                pattern: None,
                description: None,
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
            exact_action: None,
            pattern: None,
            description: None,
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
                exact_action: None,
                pattern: None,
                ..allow.clone()
            },
        )
        .unwrap();

        let loaded = load_rules(&path);
        let key = action_key_for(&path).unwrap();
        assert_eq!(loaded.len(), 2, "deduped by (tool, action)");
        let ls = loaded
            .iter()
            .find(|r| r.action == action_fingerprint(&key, "ls"))
            .unwrap();
        assert_eq!(ls.decision, Decision::Deny, "the later answer won");
        assert!(loaded.iter().all(|r| r.persist), "loaded rules are durable");
        assert!(!std::fs::read_to_string(path).unwrap().contains("\"ls\""));
    }

    #[test]
    fn loading_reconciles_repeated_exact_slots_without_merging_distinct_rules() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");
        let first = Rule {
            project: "/one".into(),
            tool: "Bash".into(),
            action: "ls".into(),
            exact_action: None,
            pattern: None,
            description: None,
            decision: Decision::Allow,
            persist: true,
            connection: None,
        };
        append_rule(&path, &first).unwrap();
        append_rule(
            &path,
            &Rule {
                action: "pwd".into(),
                exact_action: None,
                pattern: None,
                ..first.clone()
            },
        )
        .unwrap();
        append_rule(
            &path,
            &Rule {
                project: "/two".into(),
                ..first.clone()
            },
        )
        .unwrap();
        append_rule(
            &path,
            &Rule {
                tool: "WebFetch".into(),
                ..first.clone()
            },
        )
        .unwrap();

        // An encrypted file may still contain duplicate slots after an older writer.
        crate::permission_vault::mutate(&path, |stored| {
            stored.push(StoredRule {
                project: "/one".into(),
                tool: "Bash".into(),
                action: "ls".into(),
                pattern: None,
                description: None,
                decision: Decision::Deny,
            });
            Ok(())
        })
        .unwrap();

        let loaded = load_rules(&path);
        assert_eq!(loaded.len(), 4);
        assert_eq!(
            loaded
                .iter()
                .map(|r| r.project.as_str())
                .collect::<Vec<_>>(),
            vec!["/one", "/two", "/one", "/one"]
        );
        assert_eq!(
            loaded.iter().map(|r| r.tool.as_str()).collect::<Vec<_>>(),
            vec!["Bash", "Bash", "WebFetch", "Bash"]
        );
        assert_eq!(loaded.last().unwrap().decision, Decision::Deny);
        assert_eq!(
            load_rules(&path),
            loaded,
            "a second load does not add or reorder rows"
        );
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("\"ciphertext\"")
        );
        assert!(!fs::read_to_string(&path).unwrap().contains("\"ls\""));
        assert_eq!(crate::permission_vault::load(&path).unwrap().len(), 4);

        append_rule(&path, &first).unwrap();
        let replaced = load_rules(&path);
        assert_eq!(replaced.len(), 4, "repeat-add replaces the exact slot");
        assert_eq!(replaced.last().unwrap().decision, Decision::Allow);
    }

    #[test]
    fn secret_bearing_actions_remain_exact_and_are_redacted_from_debug() {
        let secret = "curl -H 'Authorization: Bearer sk-example-private' https://example.test";
        let near = "curl -H 'Authorization: Bearer sk-example-other' https://example.test";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");
        let key = action_key_for(&path).unwrap();
        let mut bridge = PermissionBridge::new();
        bridge.set_action_key_and_rules(key, Vec::new());
        let id = match bridge.request_for_project("project", "chat", "Bash", secret) {
            Outcome::Pending(id) => id,
            other => panic!("expected a pending ask, got {other:?}"),
        };
        let resolved = bridge.resolve(id, Decision::Allow, Scope::Always).unwrap();
        let rule = resolved.persisted_rule.unwrap();
        assert_eq!(rule.action, action_fingerprint(&key, secret));
        assert!(!format!("{rule:?}").contains("sk-example-private"));
        let listed = bridge
            .rules()
            .iter()
            .map(RuleInfo::from)
            .collect::<Vec<_>>();
        assert_eq!(listed[0].exact_action.as_deref(), Some(secret));
        assert!(!format!("{:?}", listed[0]).contains("sk-example-private"));
        assert_eq!(
            bridge.request_for_project("project", "other-chat", "Bash", secret),
            Outcome::Auto(Decision::Allow)
        );
        assert!(matches!(
            bridge.request_for_project("project", "other-chat", "Bash", near),
            Outcome::Pending(_)
        ));

        append_rule(&path, &rule).unwrap();
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("sk-example-private")
        );
        let mut restarted = PermissionBridge::new();
        restarted.set_action_key_and_rules(key, load_rules(&path));
        assert_eq!(
            restarted.request_for_project("project", "fresh-chat", "Bash", secret),
            Outcome::Auto(Decision::Allow)
        );
        assert!(
            restarted
                .revoke("project", None, "Bash", &listed[0].action)
                .is_some()
        );
        assert!(matches!(
            restarted.request_for_project("project", "fresh-chat", "Bash", secret),
            Outcome::Pending(_)
        ));
    }

    #[test]
    fn loading_a_legacy_rule_scrubs_raw_action_and_requires_reapproval() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");
        let secret = "API_TOKEN=private-value npm test";
        std::fs::write(
            &path,
            serde_json::to_string(&vec![StoredRule {
                project: "p".into(),
                tool: "Bash".into(),
                action: secret.into(),
                pattern: None,
                description: None,
                decision: Decision::Allow,
            }])
            .unwrap(),
        )
        .unwrap();
        let rules = load_rules(&path);
        let key = action_key_for(&path).unwrap();
        assert!(rules.is_empty());
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("private-value")
        );
        let mut bridge = PermissionBridge::new();
        bridge.set_action_key_and_rules(key, rules);
        assert!(matches!(
            bridge.request_for_project("p", "c", "Bash", secret),
            Outcome::Pending(_)
        ));
        assert!(load_rules(&path).is_empty());
    }

    #[test]
    fn exported_rule_text_cannot_confirm_an_action_guess_without_the_local_key() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let action = "PASSWORD=1234 npm test";
        let rule = Rule {
            project: "p".into(),
            tool: "Bash".into(),
            action: action.into(),
            exact_action: None,
            pattern: None,
            description: None,
            decision: Decision::Allow,
            persist: true,
            connection: None,
        };
        let first_path = first.path().join("rules.json");
        let second_path = second.path().join("rules.json");
        append_rule(&first_path, &rule).unwrap();
        append_rule(&second_path, &rule).unwrap();
        let first_export = fs::read_to_string(&first_path).unwrap();
        let second_export = fs::read_to_string(&second_path).unwrap();
        let unkeyed = format!("{:x}", Sha256::digest(action.as_bytes()));
        assert!(!first_export.contains(action));
        assert!(!first_export.contains(&unkeyed));
        assert_ne!(
            first_export, second_export,
            "different installs use different keys"
        );
        assert!(first_export.contains("\"version\":3"));
        assert!(!first_export.contains("sha256:v1:"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&first_path).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "the encrypted file is owner-only");
            assert!(
                !first.path().join(".action-key").exists(),
                "key is not stored in the file tree"
            );
        }
    }

    #[test]
    fn v1_rule_is_scrubbed_and_requires_reapproval() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");
        let v1 = format!("sha256:v1:{:x}", Sha256::digest(b"SECRET=1234 npm test"));
        fs::write(
            &path,
            serde_json::to_string(&vec![StoredRule {
                project: "p".into(),
                tool: "Bash".into(),
                action: v1.clone(),
                pattern: None,
                description: None,
                decision: Decision::Allow,
            }])
            .unwrap(),
        )
        .unwrap();
        assert!(load_rules(&path).is_empty());
        assert!(!fs::read_to_string(&path).unwrap().contains(&v1));
        let mut bridge = PermissionBridge::new();
        bridge.set_action_key_and_rules(action_key_for(&path).unwrap(), load_rules(&path));
        assert!(matches!(
            bridge.request_for_project("p", "chat", "Bash", "SECRET=1234 npm test"),
            Outcome::Pending(_)
        ));
    }

    #[test]
    fn lost_key_fails_closed_and_preserves_encrypted_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");
        let rule = Rule {
            project: "p".into(),
            tool: "Bash".into(),
            action: "npm test".into(),
            exact_action: None,
            pattern: None,
            description: None,
            decision: Decision::Allow,
            persist: true,
            connection: None,
        };
        append_rule(&path, &rule).unwrap();
        let old = load_rules(&path).pop().unwrap();
        let before = fs::read(&path).unwrap();
        crate::permission_vault::with_secrets(|store| {
            let account = crate::permission_vault::account(&path)?;
            store.delete(&account).map_err(io::Error::other)?;
            Ok(())
        })
        .unwrap();
        assert!(load_rules(&path).is_empty());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(
            append_rule(&path, &old).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let mut bridge = PermissionBridge::new();
        bridge.set_action_key_and_rules([0; 32], vec![old]);
        assert!(matches!(
            bridge.request_for_project("p", "chat", "Bash", "npm test"),
            Outcome::Pending(_)
        ));
    }

    #[test]
    fn failed_persistent_write_keeps_request_pending_and_does_not_cascade() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");
        fs::write(&path, b"corrupt").unwrap();
        let bridge = SharedPermissionBridge::new(PermissionBridge::new());
        let first = match bridge
            .inner
            .lock_or_recover()
            .request_for_project("p", "a", "Bash", "ls")
        {
            Outcome::Pending(id) => id,
            _ => panic!(),
        };
        let second = match bridge
            .inner
            .lock_or_recover()
            .request_for_project("p", "b", "Bash", "ls")
        {
            Outcome::Pending(id) => id,
            _ => panic!(),
        };
        assert!(
            bridge
                .resolve_persisted(first, Decision::Allow, None, &path)
                .is_err()
        );
        assert_eq!(
            bridge.pending().iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![first, second]
        );
        assert!(bridge.rules().is_empty());
        assert!(bridge.results.lock_or_recover().is_empty());
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
            exact_action: None,
            pattern: None,
            description: None,
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
        // An exact chat edit changes only that chat; the project answer stays intact.
        let chat_action = infos[0].action.clone();
        let project_action = infos[1].action.clone();
        assert!(
            b.update_decision("p", Some("chat-a"), "Bash", &chat_action, Decision::Deny)
                .is_some()
        );
        assert_eq!(
            b.request_for_project("p", "chat-a", "Bash", "ls"),
            Outcome::Auto(Decision::Deny)
        );
        assert_eq!(b.rules()[1].decision, Decision::Deny);
        assert!(
            b.update_decision("p", None, "Bash", &project_action, Decision::Allow)
                .is_some()
        );
        assert_eq!(
            b.request_for_project("p", "chat-c", "Bash", "ls"),
            Outcome::Auto(Decision::Allow)
        );
        assert!(
            b.update_decision("q", None, "Bash", &project_action, Decision::Deny)
                .is_none()
        );
        // Restore the original decisions for the rest of this transition matrix.
        b.update_decision("p", Some("chat-a"), "Bash", &chat_action, Decision::Allow);
        b.update_decision("p", None, "Bash", &project_action, Decision::Deny);
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
            exact_action: None,
            pattern: None,
            description: None,
            decision: Decision::Allow,
            persist: true,
            connection: None,
        };
        append_rule(&path, &rule).unwrap();
        append_rule(
            &path,
            &Rule {
                action: "pwd".into(),
                exact_action: None,
                pattern: None,
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

    #[test]
    fn restoring_durable_rules_preserves_chat_session_and_pending_state() {
        let mut bridge = PermissionBridge::new();
        let chat_id = pending(bridge.request_for_project("p", "chat-a", "Bash", "chat-action"));
        bridge
            .resolve(chat_id, Decision::Deny, Scope::Chat)
            .unwrap();
        let session_id =
            pending(bridge.request_for_project("p", "chat-a", "Bash", "session-action"));
        bridge
            .resolve(session_id, Decision::Allow, Scope::Session)
            .unwrap();
        let waiting = pending(bridge.request_for_project("p", "chat-a", "Bash", "waiting"));
        let durable = Rule {
            project: "p".into(),
            tool: "Bash".into(),
            action: "durable-action".into(),
            exact_action: None,
            pattern: None,
            description: None,
            decision: Decision::Allow,
            persist: true,
            connection: None,
        };
        bridge.restore_durable_rules(fresh_action_key().unwrap(), vec![durable]);
        assert_eq!(
            bridge
                .pending()
                .map(|request| request.id)
                .collect::<Vec<_>>(),
            [waiting]
        );
        assert_eq!(bridge.rules().len(), 3);
        assert_eq!(
            bridge.request_for_project("p", "chat-a", "Bash", "chat-action"),
            Outcome::Auto(Decision::Deny)
        );
        assert_eq!(
            bridge.request_for_project("p", "chat-b", "Bash", "session-action"),
            Outcome::Auto(Decision::Allow)
        );
        assert_eq!(
            bridge.request_for_project("p", "chat-b", "Bash", "durable-action"),
            Outcome::Auto(Decision::Allow)
        );
        bridge.restore_durable_rules(fresh_action_key().unwrap(), Vec::new());
        assert_eq!(bridge.rules().len(), 2);
        assert_eq!(
            bridge
                .pending()
                .map(|request| request.id)
                .collect::<Vec<_>>(),
            [waiting]
        );
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
            exact_action: None,
            pattern: None,
            description: None,
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
