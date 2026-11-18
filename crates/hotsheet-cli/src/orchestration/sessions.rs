//! HS2-GDJXGZ: AI sessions the orchestrator starts on the servers that host its targets.
//!
//! The hosted half runs where the target lives. It reserves a terminal for an
//! operation id, asks its own admission, launches through the server's ordinary AI
//! terminal path, labels the terminal with the orchestration project and run, and is
//! the only side that can say whether a session is at a safe point. The orchestrating
//! half passes the gate, keeps this machine's record of what it asked for, and never
//! stops a session the host does not confirm as safe unless a person says so.
use hotsheet_ticketing::actor::{ActorRole, MutationActor};
use hotsheet_ticketing::ops;
use hotsheet_ticketing::orchestration::{
    AdmissionOutcome, AdmissionRequest, AuditOutcome, AuditRecord, DenialReason, ExternalWork,
    HostedRef, HostedSession, HostedSessionRegistry, MachineLimits, NewHostedSession, NewSession,
    Observed, OrchestratedSession, RunEvent, RunEventKind, RunEventSource, SafePoint,
    SafePointFacts, SessionControl, SessionError, SessionLoad, SessionOrigin, SessionOwner,
    SessionRegistry, SessionRequest, SessionRole, SessionState, SessionUpdate, StopRequest,
    TargetAction, TargetDestination, TargetOpError, Withdrawal, decide_safe_point, decide_stop,
};
pub use hotsheet_ticketing::orchestration::{PushAttempt, RequestKind, RolePrompt};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::tickets::{encode, identifier, timestamp};
use super::*;

// ---- The terminal host ----

/// What a host is asked to launch. The id is the terminal the host reserved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostLaunch {
    pub session_id: String,
    pub tool: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The drawer label: the orchestration project and its run or role.
    pub label: String,
    /// HS2-2DPGDY: the role the session takes up. The host names it in the session's
    /// environment (`HOTSHEET_SESSION_ROLE`, `HOTSHEET_ROLE_SKILL`).
    pub role: SessionRole,
    /// HS2-2DPGDY: the role prompt. A tool whose plugin declares `[launch]
    /// prompt_args` receives it as its first prompt at launch.
    pub role_prompt: String,
}

/// HS2-2DPGDY: what a launch did beyond starting the tool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Launched {
    /// The role prompt went with the launch as the tool's first prompt.
    pub prompt_at_launch: bool,
}

/// HS2-2DPGDY: whether a plugin takes a first prompt at launch, and whether it can
/// take a message from its host at all. `None` when the plugin is unknown.
pub fn plugin_role_paths(tool: &str, plugin_dirs: &[PathBuf]) -> Option<(bool, bool)> {
    let plugin = hotsheet_plugins::find_in(tool, plugin_dirs)?;
    let at_launch = plugin
        .manifest
        .launch
        .as_ref()
        .is_some_and(|launch| !launch.prompt_args.is_empty());
    Some((at_launch, plugin.manifest.session_push.is_some()))
}

/// One live AI session as its host sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSessionState {
    pub session_id: String,
    pub busy: bool,
    /// HS2-RG91SK: since when the session has been idle (its last turn finished), as the
    /// host measured it; `None` while busy or from a host that does not say.
    pub idle_since_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionHostError {
    /// No terminal host could be reached.
    Unavailable,
    /// The host answered and would not launch or stop it.
    Refused,
}

/// HS2-WEFJV8: what the target project's own AI-tool setup did before a launch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSetup {
    /// Files the setup wrote in or for the checkout.
    pub wrote: u32,
}

/// HS2-WEFJV8: the store a checkout's AI-tool setup points at. A linked Git source is
/// the project's own, chosen as the project-open refresh chooses it; a checkout
/// without one uses the store of the server that hosts it.
pub fn setup_store(checkout: &Checkout, serving_store: Option<&Path>) -> Option<PathBuf> {
    checkout
        .sources
        .iter()
        .find(|source| source.provider == "git")
        .map(|source| PathBuf::from(&source.locator))
        .or_else(|| serving_store.map(Path::to_path_buf))
}

/// HS2-WEFJV8: prepare the target checkout for `tool` with the project's own headless
/// setup ([`crate::setup::run_setup_in`]), so the session finds its MCP, instruction
/// and hook configuration in the directory it starts in. Every host calls this one
/// function; there is no second setup workflow.
pub fn target_tool_setup(
    checkout: &Checkout,
    tool: &str,
    serving_store: Option<&Path>,
    plugin_dirs: &[PathBuf],
) -> Result<ToolSetup, SessionHostError> {
    let project = Path::new(&checkout.root);
    hotsheet_aitools::launch_safety::assert_no_hs1(project)
        .map_err(|_| SessionHostError::Refused)?;
    let store = setup_store(checkout, serving_store).ok_or(SessionHostError::Refused)?;
    let reports = crate::setup::run_setup_in(&store, project, Some(tool), false, plugin_dirs)
        .map_err(|_| SessionHostError::Refused)?;
    Ok(ToolSetup {
        wrote: u32::try_from(
            reports
                .iter()
                .map(|report| report.wrote.len())
                .sum::<usize>(),
        )
        .unwrap_or(u32::MAX),
    })
}

/// The process that owns AI terminals. The server injects itself; a headless process
/// reaches the server running on this machine; tests script one.
pub trait SessionHost: Send + Sync {
    /// HS2-WEFJV8: run the target project's own setup for the tool in the checkout,
    /// before anything launches there. A setup that cannot be made refuses the start.
    fn prepare(&self, checkout: &Checkout, tool: &str) -> Result<ToolSetup, SessionHostError>;

    /// Launch the tool in the checkout through the ordinary AI terminal path. Launching
    /// an id that already runs attaches to it and starts nothing.
    fn start(&self, checkout: &Checkout, launch: &HostLaunch)
    -> Result<Launched, SessionHostError>;

    /// Every live AI session of this host. `None` when the host could not be asked, so
    /// a failed listing never reads as "nothing is running".
    fn sessions(&self, checkout: &Checkout) -> Option<Vec<HostSessionState>>;

    fn stop(&self, checkout: &Checkout, session_id: &str) -> Result<(), SessionHostError>;

    /// Push the role prompt into the running session once. HS2-2DPGDY: a tool that
    /// is not ready yet answers `NotReady`, and the host tries again on its next
    /// readiness event.
    fn join_role(&self, checkout: &Checkout, session_id: &str, text: &str) -> PushAttempt;

    /// HS2-VRC0CQ: the process that hosts these sessions is shutting down. Its
    /// scheduler places nothing more, including from a step that was already under
    /// way when the shutdown began.
    fn stopping(&self) -> bool {
        false
    }
}

/// No terminal host is reachable from this process.
pub struct NoSessionHost;
impl SessionHost for NoSessionHost {
    fn prepare(&self, _: &Checkout, _: &str) -> Result<ToolSetup, SessionHostError> {
        Err(SessionHostError::Unavailable)
    }
    fn start(&self, _: &Checkout, _: &HostLaunch) -> Result<Launched, SessionHostError> {
        Err(SessionHostError::Unavailable)
    }
    fn sessions(&self, _: &Checkout) -> Option<Vec<HostSessionState>> {
        None
    }
    fn stop(&self, _: &Checkout, _: &str) -> Result<(), SessionHostError> {
        Err(SessionHostError::Unavailable)
    }
    fn join_role(&self, _: &Checkout, _: &str, _: &str) -> PushAttempt {
        PushAttempt::NotReady
    }
}

/// HS2-WEFJV8: the hosted refusal when the target checkout could not be set up for the
/// tool. Nothing was launched.
pub const TOOL_SETUP_FAILED: &str = "tool_setup_failed";

/// The message that asks a started session to take up its role. It names the skill;
/// the skill itself is the workflow. HS2-2DPGDY: it names the worker id to register
/// in the target's coordination pool with, which is how joining is confirmed, and the
/// exact ticket to request when the session was started for one.
pub fn role_prompt_text(role: SessionRole, worker_id: &str, request: Option<&str>) -> String {
    let mut text = format!(
        "Hot Sheet orchestration started this session as a {} for this project. Run the `{}` skill and follow its workflow, registering in the coordination pool with the id `{worker_id}`.",
        role.code(),
        role.skill()
    );
    if let Some(ticket) = request {
        text.push_str(&format!(
            " Request the ticket `{ticket}` from the dispatcher as your argument to the skill."
        ));
    }
    text
}

/// HS2-7YSKXE: what a session is told about the resources its work needs. It acquires
/// each itself with the claim service, keeps the renewal token, renews while it works
/// and releases when done; the `hotsheet-worker` skill teaches the pattern. `None`
/// when the work needs none.
pub fn resources_prompt_text(
    resources: &[hotsheet_ticketing::orchestration::ResourceNeed],
) -> Option<String> {
    if resources.is_empty() {
        return None;
    }
    let names: Vec<String> = resources
        .iter()
        .map(|need| format!("`{}/{}`", need.namespace, need.resource))
        .collect();
    Some(format!(
        "This work needs the resources {}. Before you start it, acquire each yourself with `hotsheet-cli resource acquire <namespace>/<resource> --duration <how long the work takes> --wait 5m` (or the `hotsheet_resource_claim` tool), keep the renewal token, renew before the lease ends while you still work, and release each one as soon as you are done. Nobody else claims them for you; follow the resource claims section of the `hotsheet-worker` skill.",
        names.join(", ")
    ))
}

/// HS2-HCW7MD: the message that asks a planning session to propose the plan. It
/// names the skill, the orchestration project's checkout id every orchestration
/// command takes, the proposal's id, and the one goal ticket when only one was asked
/// for. Nothing in it is content: identifiers only.
pub fn planner_prompt_text(
    owner_checkout_id: &str,
    worker_id: &str,
    proposal_id: &str,
    goal: Option<&str>,
) -> String {
    let goals = goal.map_or_else(
        || "every open goal ticket of this project".to_owned(),
        |goal| format!("the goal ticket `{goal}`"),
    );
    format!(
        "Hot Sheet orchestration started this session as the planner of this orchestration project (`{owner_checkout_id}`). Run the `{}` skill and follow its workflow to propose the dry-run plan for {goals}, as the worker `{worker_id}`, for the proposal `{proposal_id}`. Do not dispatch runs, start sessions or change the policy.",
        SessionRole::Planner.skill()
    )
}

/// HS2-R7QH35: the message that tells a running session which exact ticket its new
/// run is for. `ticket` is the slug where the host can read it.
pub fn request_prompt_text(
    kind: RequestKind,
    worker_id: &str,
    ticket: &str,
    commit: Option<&str>,
) -> String {
    match kind {
        RequestKind::Assigned => format!(
            "Hot Sheet orchestration gave this session new work: the ticket `{ticket}`. Request it from the dispatcher with the `hotsheet-worker` skill, as the worker `{worker_id}`."
        ),
        RequestKind::Claimed => format!(
            "Hot Sheet orchestration found no live dispatcher for this project, so the ticket `{ticket}` is claimed for you under your worker id `{worker_id}`. Work it with the `hotsheet-worker` skill, renewing that claim, without waiting for a dispatcher."
        ),
        RequestKind::Cancelled => format!(
            "A person cancelled the orchestration run for the ticket `{ticket}`. Stop working it: commit or set aside what you have, release your claim on it as the worker `{worker_id}`, and finish your turn. Hot Sheet will wind this session down."
        ),
        RequestKind::Rebase => format!(
            "Your work on the ticket `{ticket}` was started ahead of a prerequisite's CI gate, which failed. Its fix is the commit `{}`. Rebase your work onto that commit, rerun the gates the change affects, and keep the work off the shipping branch until they pass. Continue as the worker `{worker_id}`.",
            commit.unwrap_or("unknown")
        ),
    }
}

/// HS2-2DPGDY: whether the target's pool shows the session registered, unexpired, in
/// the role it was started for. `now_s` is in seconds, as the pool counts.
pub fn pool_joined(
    state: &crate::coordination::State,
    role: SessionRole,
    worker_id: &str,
    now_s: i64,
) -> bool {
    let live = |session: &crate::coordination::Session| {
        session.id == worker_id && session.expires_at > now_s
    };
    match role {
        SessionRole::Worker => state.workers.get(worker_id).is_some_and(live),
        SessionRole::Dispatcher => state.dispatcher.as_ref().is_some_and(live),
        SessionRole::Integrator => state.integrator.as_ref().is_some_and(live),
        // HS2-HCW7MD: the planner joins no pool.
        SessionRole::Planner => false,
    }
}

/// A headless process hosts no terminals, so it uses the ordinary terminal routes of
/// the Hot Sheet server running on this machine. Without one, nothing can be started.
pub struct LocalServerSessions {
    home: PathBuf,
}
impl LocalServerSessions {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }

    fn servers(&self) -> Vec<crate::external_launch::ServerInstance> {
        crate::external_launch::running_servers(&self.home)
    }

    fn call(
        server: &crate::external_launch::ServerInstance,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, Option<u16>> {
        let request = ureq::request(
            method,
            &format!("{}{path}", server.url.trim_end_matches('/')),
        )
        .set("X-Hotsheet-Secret", &server.secret)
        .timeout(std::time::Duration::from_secs(60));
        let response = match body {
            Some(body) => request
                .set("Content-Type", "application/json")
                .send_string(&body.to_string()),
            None => request.call(),
        };
        match response {
            Ok(response) => {
                Ok(serde_json::from_reader(response.into_reader()).unwrap_or(Value::Null))
            }
            Err(ureq::Error::Status(status, _)) => Err(Some(status)),
            Err(_) => Err(None),
        }
    }
}
impl SessionHost for LocalServerSessions {
    fn prepare(&self, checkout: &Checkout, tool: &str) -> Result<ToolSetup, SessionHostError> {
        // The headless setup itself, in this process, pointing a checkout without a
        // linked store at the store of the server that will host the terminal.
        let servers = self.servers();
        let Some(server) = servers.first() else {
            return Err(SessionHostError::Unavailable);
        };
        target_tool_setup(
            checkout,
            tool,
            Some(Path::new(&server.store_path)),
            &hotsheet_plugins::default_dirs(),
        )
    }
    fn start(
        &self,
        checkout: &Checkout,
        launch: &HostLaunch,
    ) -> Result<Launched, SessionHostError> {
        let mut answered = false;
        let prompt_at_launch = plugin_role_paths(&launch.tool, &hotsheet_plugins::default_dirs())
            .is_some_and(|(at_launch, _)| at_launch);
        for server in self.servers() {
            let body = serde_json::json!({
                "connect": launch.tool,
                "id": launch.session_id,
                "cwd": checkout.root,
                "model": launch.model,
                "effort": launch.effort,
                "session_role": launch.role.code(),
                "initial_prompt": launch.role_prompt,
            });
            match Self::call(&server, "POST", "/terminals", Some(body)) {
                Ok(_) => {
                    // The label is the saved tab name every client of that server shows.
                    let _ = Self::call(
                        &server,
                        "PUT",
                        &format!("/terminals/{}/name", launch.session_id),
                        Some(serde_json::json!({"name": launch.label})),
                    );
                    return Ok(Launched { prompt_at_launch });
                }
                Err(Some(_)) => answered = true,
                Err(None) => {}
            }
        }
        Err(if answered {
            SessionHostError::Refused
        } else {
            SessionHostError::Unavailable
        })
    }

    fn sessions(&self, _: &Checkout) -> Option<Vec<HostSessionState>> {
        let mut live = Vec::new();
        let mut answered = false;
        for server in self.servers() {
            let Ok(Value::Array(terminals)) = Self::call(&server, "GET", "/terminals", None) else {
                continue;
            };
            answered = true;
            for terminal in terminals {
                let field = |key: &str| terminal.get(key);
                if field("kind").and_then(Value::as_str) != Some("ai")
                    || field("alive").and_then(Value::as_bool) != Some(true)
                {
                    continue;
                }
                if let Some(id) = field("id").and_then(Value::as_str) {
                    live.push(HostSessionState {
                        session_id: id.into(),
                        busy: field("busy").and_then(Value::as_bool).unwrap_or(true),
                        idle_since_ms: field("idle_since_ms").and_then(Value::as_u64),
                    });
                }
            }
        }
        answered.then_some(live)
    }

    fn stop(&self, _: &Checkout, session_id: &str) -> Result<(), SessionHostError> {
        let mut answered = false;
        for server in self.servers() {
            match Self::call(&server, "DELETE", &format!("/terminals/{session_id}"), None) {
                Ok(_) => return Ok(()),
                Err(Some(_)) => answered = true,
                Err(None) => {}
            }
        }
        Err(if answered {
            SessionHostError::Refused
        } else {
            SessionHostError::Unavailable
        })
    }

    fn join_role(&self, _: &Checkout, session_id: &str, text: &str) -> PushAttempt {
        let mut outcome = PushAttempt::NotReady;
        for server in self.servers() {
            match Self::call(
                &server,
                "POST",
                &format!("/connections/{session_id}/message"),
                Some(serde_json::json!({"text": text})),
            ) {
                Ok(_) => return PushAttempt::Delivered,
                // The server has no address for it: one is coming when its tool can
                // take messages at all, and never otherwise.
                Err(Some(422)) => {
                    let pushes = Self::call(&server, "GET", "/terminals", None)
                        .ok()
                        .and_then(|terminals| {
                            terminals.as_array()?.iter().find_map(|terminal| {
                                (terminal["id"] == session_id)
                                    .then(|| terminal["tool"].as_str().map(str::to_owned))
                                    .flatten()
                            })
                        })
                        .and_then(|tool| {
                            plugin_role_paths(&tool, &hotsheet_plugins::default_dirs())
                        })
                        .is_some_and(|(_, pushes)| pushes);
                    outcome = if pushes {
                        PushAttempt::NotReady
                    } else {
                        PushAttempt::Unsupported
                    };
                }
                Err(Some(400)) => outcome = PushAttempt::Rejected,
                // Not listening yet, or not registered yet: maybe later.
                Err(_) => {}
            }
        }
        outcome
    }
}

// ---- Wire types ----

/// The hosted start: what to launch and the orchestrator's machine limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedSessionStart {
    pub session: NewHostedSession,
    /// The policy's machine limits for this target; the host's own file may only be
    /// stricter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<MachineLimits>,
    /// Who asked, for attribution. The start is decided by this machine either way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<MutationActor>,
    /// HS2-7YSKXE: resources the work needs; the started session is told to acquire
    /// them itself. Absent when there are none, so an older host reads the start.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<hotsheet_ticketing::orchestration::ResourceNeed>,
}

/// A hosted session with what its host knows about it right now.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedSessionView {
    pub session: HostedSession,
    pub server_id: String,
    /// The worker id its claims and assignments carry.
    pub worker_id: String,
    /// The name of its tab in the hosting server's drawer.
    pub label: String,
    /// `null` when the host could not be asked.
    pub alive: Option<bool>,
    pub busy: Option<bool>,
    /// Present when the caller asked for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_point: Option<SafePoint>,
    /// HS2-2DPGDY: how the session was asked to take up its role, as of now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_prompt: Option<RolePrompt>,
    /// HS2-2DPGDY: the target's coordination pool shows the session registered in
    /// its role. `null` when the pool could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joined: Option<bool>,
    /// HS2-R7QH35: how the latest request naming one ticket stands, as of now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_prompt: Option<RolePrompt>,
}

/// HS2-R7QH35: tell a running hosted session which exact ticket it now works.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedSessionRequest {
    pub operation_id: String,
    /// The ticket's canonical id in the target checkout.
    pub ticket: String,
    pub kind: RequestKind,
    /// HS2-2K0DM8: the fix commit a `rebase` request names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<MutationActor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedRequestReply {
    /// How the request stands; absent when it was refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_prompt: Option<RolePrompt>,
    /// Why not, as a session code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostedStartOutcome {
    Started,
    /// An earlier request with the same operation id already started it.
    Existing,
    Refused,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedStartReply {
    pub outcome: HostedStartOutcome,
    /// Why the host refused: an admission reason or a session code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<HostedSessionView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_prompt: Option<RolePrompt>,
    /// HS2-WEFJV8: what the checkout's tool setup wrote before this launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_setup: Option<ToolSetup>,
}

/// Whether the target's pool already has a live session in each single-holder role.
/// `null` when the pool could not be read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedRoles {
    pub integrator_live: Option<bool>,
    pub dispatcher_live: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedSessionList {
    pub sessions: Vec<HostedSessionView>,
    pub roles: HostedRoles,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedSessionStop {
    pub operation_id: String,
    /// Stop even when the session is not at a safe point. A person's request only.
    #[serde(default)]
    pub force: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<MutationActor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedStopReply {
    pub stopped: bool,
    /// Why not, as a session code; `session_not_found` when the host has no record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_point: Option<SafePoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<HostedSessionView>,
    /// HS2-WHR4GS: the host fenced the operation, so a start of it arriving later
    /// launches nothing. Absent from a host without the fence.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fenced: bool,
}

/// A person takes a hosted session over, optionally pausing it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedSessionControl {
    pub operation_id: String,
    #[serde(default)]
    pub pause: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<MutationActor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedControlReply {
    /// Absent when the host has no record of the operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<HostedSessionView>,
}

/// HS2-DX1PQB: where a hosted session's owner, pause or end changing is signalled.
pub const HOSTED_SESSION_SIGNAL_DIRECTORY: &str = "orchestration-hosted-session-signal";

/// The orchestrating start request.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartSession {
    pub target_id: String,
    /// Retain across retries: the same id is the same session.
    pub operation_id: String,
    pub tool: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    /// Required when the policy configures accounts for the tool.
    #[serde(default)]
    pub account: Option<String>,
    /// `worker` (the default), `dispatcher` or `integrator`.
    #[serde(default)]
    pub role: Option<SessionRole>,
    /// The run the session is started for, when it serves one.
    #[serde(default)]
    pub run_id: Option<String>,
    /// The lost or failed session this one replaces.
    #[serde(default)]
    pub replaces: Option<String>,
    /// HS2-7YSKXE: resources the work needs, at most four, as `{namespace, resource}`.
    /// The session acquires them itself; nobody claims on its behalf.
    #[serde(default)]
    pub resources: Vec<hotsheet_ticketing::orchestration::ResourceNeed>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopSession {
    /// Stop even when the session is not at a safe point. A person's request only.
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TakeoverSession {
    /// Also pause it: the orchestrator gives it nothing new.
    #[serde(default)]
    pub pause: bool,
}

/// A session with what this answer learned from its host.
#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    #[serde(flatten)]
    pub session: OrchestratedSession,
    /// The name of its tab in the hosting server's drawer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role_prompt: Option<RolePrompt>,
    /// HS2-R7QH35: how the request naming the session's ticket stands, when this
    /// answer sent one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_prompt: Option<RolePrompt>,
    /// HS2-WEFJV8: what the target checkout's tool setup wrote before the launch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_setup: Option<ToolSetup>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe_point: Option<SafePoint>,
    /// Names of external state this answer could not verify, such as `target_session`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unverified: Vec<&'static str>,
}

impl From<OrchestratedSession> for SessionView {
    fn from(session: OrchestratedSession) -> Self {
        Self {
            session,
            label: None,
            role_prompt: None,
            request_prompt: None,
            tool_setup: None,
            safe_point: None,
            unverified: Vec::new(),
        }
    }
}

/// One session a bulk sync could not examine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionSyncFailure {
    pub session_id: String,
    pub code: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSyncSummary {
    pub sessions: Vec<SessionView>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<SessionSyncFailure>,
}

/// HS2-95SWQ4: how the gate sees a start or a stop the scheduler placed. The
/// scheduler asked the side-effect-free check under its own operation id, and a
/// person's permission answer is for that id, so the act passes the gate under it too.
pub(super) struct PlacedGate<'a> {
    pub(super) operation_id: &'a str,
    /// The sessions the scheduler counted when it decided a start.
    pub(super) load: Option<&'a SessionLoad>,
}

fn is_human(actor: Option<&MutationActor>) -> bool {
    actor.is_some_and(|actor| actor.role == ActorRole::Human)
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = hotsheet_ticketing::git::launch("git")
        // HS2-95SWQ4: a scheduler pass reads the checkout while a person or a session
        // may be committing in it. A read must never hold the index lock they need.
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// How many of the worktrees a session owns have uncommitted changes, untracked
/// files included (HS2-TRP2MJ, `session_worktrees`). `None` when the checkout is not
/// a git work tree, git could not say, or a worker's pool could not be read.
fn dirty_worktrees(root: &Path, role: SessionRole, worker: &str) -> Option<u32> {
    let listing = git(root, &["worktree", "list", "--porcelain"])?;
    let worktrees = super::session_worktrees::parse_worktrees(&listing);
    let state = match role {
        SessionRole::Worker => coordination_state(root).ok()?,
        _ => None,
    };
    let assignments: Vec<&crate::coordination::Assignment> = state
        .as_ref()
        .map(|state| state.assignments.values().collect())
        .unwrap_or_default();
    let mut dirty = 0;
    for worktree in
        super::session_worktrees::owned_worktrees(&worktrees, role, worker, &assignments)
    {
        if !git(&worktree.path, &["status", "--porcelain"])?
            .trim()
            .is_empty()
        {
            dirty += 1;
        }
    }
    Some(dirty)
}

/// The checkout's coordination pool, read without creating or upgrading it.
/// `Ok(None)` when the project has no pool.
fn coordination_state(root: &Path) -> Result<Option<crate::coordination::State>, ()> {
    let common = git(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok_or(())?;
    let path = Path::new(common.trim())
        .join("hotsheet-coordination")
        .join("state.json");
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|_| ()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(()),
    }
}

/// HS2-TK9ADC: what the checkout's coordination pool says of its single-holder roles
/// and of the work handed off and not yet done. `None` when it could not be read; an
/// absent pool is an empty one.
pub(super) fn hosted_pool(
    root: &Path,
    now_ms: u64,
) -> Option<hotsheet_ticketing::orchestration::PoolState> {
    use crate::coordination::Phase;
    let now = i64::try_from(now_ms / 1000).unwrap_or(i64::MAX);
    let Some(state) = coordination_state(root).ok()? else {
        return Some(hotsheet_ticketing::orchestration::PoolState::default());
    };
    let handed_off = state
        .assignments
        .values()
        .filter(|assignment| {
            matches!(
                assignment.phase,
                Phase::Handoff
                    | Phase::Verifying
                    | Phase::Integrating
                    | Phase::SuspendedIntegration
                    | Phase::Ci
            )
        })
        .count();
    Some(hotsheet_ticketing::orchestration::PoolState {
        dispatcher_live: state.dispatcher.is_some_and(|lease| lease.expires_at > now),
        integrator_live: state.integrator.is_some_and(|lease| lease.expires_at > now),
        handoffs_pending: u32::try_from(handed_off).unwrap_or(u32::MAX),
    })
}

impl OrchestrationRuntime {
    /// Replace the process that owns AI terminals (the session-hosting server, tests).
    #[must_use]
    pub fn with_session_host(mut self, host: Arc<dyn SessionHost>) -> Self {
        self.sessions = host;
        self
    }

    // ---- Hosted half ----

    fn hosted_sessions_registry(&self) -> HostedSessionRegistry {
        HostedSessionRegistry::new(&self.home)
    }

    /// HS2-DX1PQB: the directory a runtime touches when a person changes a hosted
    /// session's owner, pause or end, so every server on this machine tells its
    /// clients, whichever process (a server or a headless CLI) made the change.
    pub fn hosted_session_signal_dir(&self) -> PathBuf {
        self.home.join(HOSTED_SESSION_SIGNAL_DIRECTORY)
    }

    /// HS2-DX1PQB: a waiter on the hosted-session signal.
    pub fn hosted_session_signal_waiter(&self) -> std::io::Result<super::SignalWaiter> {
        super::SignalWaiter::at(&self.hosted_session_signal_dir())
    }

    fn hosted_session_signal(&self) {
        super::permissions::touch_signal(&self.hosted_session_signal_dir());
    }

    /// Live ticket claims held under `worker` in the checkout's git stores. `None`
    /// when a store could not be read.
    fn session_claims(
        &self,
        checkout: &Checkout,
        worker: &str,
        tx: &Transaction<'_>,
    ) -> Option<u32> {
        let now = timestamp(tx);
        let mut held = 0;
        for source in checkout
            .sources
            .iter()
            .filter(|source| source.provider == "git")
        {
            let store = checkout.verified_git_source(&source.connection_id).ok()?;
            let claimed = ops::query(
                &store,
                &ops::TicketQuery {
                    claimed: Some(true),
                    ..ops::TicketQuery::default()
                },
            )
            .ok()?;
            held += claimed
                .iter()
                .filter(|ticket| {
                    ticket.claimed_by.as_deref() == Some(worker)
                        && !ops::claim_available(ticket, &now)
                })
                .count();
        }
        u32::try_from(held).ok()
    }

    /// Assignments of the pool this session still has a part in. A worker answers for
    /// its own; the dispatcher and the integrator answer for every unfinished one.
    fn pending_handoffs(checkout: &Checkout, session: &HostedSession, worker: &str) -> Option<u32> {
        let Some(state) = coordination_state(Path::new(&checkout.root)).ok()? else {
            return Some(0);
        };
        let pending = state
            .assignments
            .values()
            .filter(|assignment| assignment.phase != crate::coordination::Phase::Done)
            .filter(|assignment| session.role.is_role_session() || assignment.worker == worker)
            .count();
        u32::try_from(pending).ok()
    }

    fn hosted_roles(checkout: &Checkout, now_ms: u64) -> HostedRoles {
        let now = i64::try_from(now_ms / 1000).unwrap_or(i64::MAX);
        match coordination_state(Path::new(&checkout.root)) {
            Ok(Some(state)) => HostedRoles {
                integrator_live: Some(state.integrator.is_some_and(|s| s.expires_at > now)),
                dispatcher_live: Some(state.dispatcher.is_some_and(|s| s.expires_at > now)),
            },
            Ok(None) => HostedRoles {
                integrator_live: Some(false),
                dispatcher_live: Some(false),
            },
            Err(()) => HostedRoles::default(),
        }
    }

    fn hosted_view(
        &self,
        checkout: &Checkout,
        session: HostedSession,
        live: Option<&[HostSessionState]>,
        with_safe_point: bool,
        tx: &Transaction<'_>,
    ) -> HostedSessionView {
        let worker_id = hotsheet_aitools::session_worker_id(&session.tool, &session.session_id);
        let state = live.map(|live| {
            live.iter()
                .find(|state| state.session_id == session.session_id)
        });
        let ended = session.ended_at_ms.is_some() || !session.started;
        let alive = if ended {
            Some(false)
        } else {
            state.map(|state| state.is_some())
        };
        let busy = match (alive, state) {
            (Some(false), _) => Some(false),
            (_, Some(Some(state))) => Some(state.busy),
            _ => None,
        };
        let safe_point = with_safe_point.then(|| {
            decide_safe_point(&if alive == Some(false) {
                SafePointFacts {
                    alive,
                    busy,
                    ..SafePointFacts::default()
                }
            } else {
                SafePointFacts {
                    alive,
                    busy,
                    held_claims: self.session_claims(checkout, &worker_id, tx),
                    dirty_worktrees: dirty_worktrees(
                        Path::new(&checkout.root),
                        session.role,
                        &worker_id,
                    ),
                    pending_handoffs: Self::pending_handoffs(checkout, &session, &worker_id),
                    // HS2-RG91SK: the host's own time of the last finished turn.
                    idle_since_ms: state
                        .flatten()
                        .filter(|state| !state.busy)
                        .and_then(|state| state.idle_since_ms),
                }
            })
        });
        let now_ms = tx.clock.now_ms();
        let joined = match coordination_state(Path::new(&checkout.root)) {
            // HS2-HCW7MD: a planner joins no pool, so there is nothing to confirm.
            _ if !session.started || session.role == SessionRole::Planner => None,
            Ok(Some(state)) => Some(pool_joined(
                &state,
                session.role,
                &worker_id,
                i64::try_from(now_ms / 1000).unwrap_or(i64::MAX),
            )),
            Ok(None) => Some(false),
            Err(()) => None,
        };
        HostedSessionView {
            label: session.label(),
            server_id: self.server_id.clone(),
            role_prompt: session.role_delivery.at(session.created_at_ms, now_ms),
            request_prompt: session
                .request
                .as_ref()
                .and_then(|request| request.at(now_ms)),
            joined,
            worker_id,
            alive,
            busy,
            safe_point,
            session,
        }
    }

    /// A started session whose terminal is gone ended without anyone stopping it.
    fn note_lost(
        &self,
        session: HostedSession,
        live: Option<&[HostSessionState]>,
        now_ms: u64,
    ) -> Result<HostedSession, OrchestrationError> {
        let gone = session.started
            && session.ended_at_ms.is_none()
            && live.is_some_and(|live| {
                !live
                    .iter()
                    .any(|state| state.session_id == session.session_id)
            });
        if !gone {
            return Ok(session);
        }
        let ended = self
            .hosted_sessions_registry()
            .end(
                &session.checkout_id,
                &session.operation_id,
                "host_lost",
                now_ms,
            )?
            .unwrap_or(session);
        // HS2-CRBR8X: the host's journal records the end once, whoever noticed it.
        self.journal_hosted_end(&ended);
        Ok(ended)
    }

    /// The sessions orchestrators started on one checkout this machine hosts, with
    /// what the terminal host says about each now.
    pub fn hosted_sessions(
        &self,
        checkout_id: &str,
        operation_id: Option<&str>,
        with_safe_point: bool,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedSessionList, OrchestrationError> {
        let checkout = self.owner(checkout_id)?;
        if let Some(operation_id) = operation_id {
            identifier(operation_id, "bounded operation id required")?;
        }
        let live = self.sessions.sessions(&checkout);
        let now_ms = tx.clock.now_ms();
        let mut sessions = Vec::new();
        for session in self.hosted_sessions_registry().list(&checkout.id)? {
            if operation_id.is_some_and(|wanted| wanted != session.operation_id) {
                continue;
            }
            let session = self.note_lost(session, live.as_deref(), now_ms)?;
            sessions.push(self.hosted_view(
                &checkout,
                session,
                live.as_deref(),
                with_safe_point,
                tx,
            ));
        }
        Ok(HostedSessionList {
            sessions,
            roles: Self::hosted_roles(&checkout, now_ms),
        })
    }

    /// Start a session on this machine for an orchestration project. The same
    /// operation id never launches twice. This machine's own admission decides.
    /// [`Self::hosted_session_start`] journals the answer (HS2-CRBR8X).
    pub(super) fn hosted_session_start_unjournaled(
        &self,
        checkout_id: &str,
        request: HostedSessionStart,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedStartReply, OrchestrationError> {
        request.session.validate()?;
        let checkout = self.owner(checkout_id)?;
        // HS2-HCW7MD: a planner runs only in the orchestration project it plans for.
        if request.session.role == SessionRole::Planner
            && request.session.origin.owner_checkout_id != checkout.id
        {
            return Err(OrchestrationError::Invalid(
                "a planner runs in its own orchestration project",
            ));
        }
        let registry = self.hosted_sessions_registry();
        let withdrawn = || HostedStartReply {
            outcome: HostedStartOutcome::Refused,
            reason: Some(SessionError::Withdrawn.code().into()),
            session: None,
            role_prompt: None,
            tool_setup: None,
        };
        let (reserved, _) =
            match registry.reserve_with(&checkout.id, &request.session, &request.resources, tx) {
                Ok(reserved) => reserved,
                // HS2-WHR4GS: a stop withdrew this operation before its start arrived.
                Err(OrchestrationError::Session(SessionError::Withdrawn)) => return Ok(withdrawn()),
                Err(error) => return Err(error),
            };
        let live = self.sessions.sessions(&checkout);
        if reserved.started {
            let session = self.note_lost(reserved, live.as_deref(), tx.clock.now_ms())?;
            return Ok(HostedStartReply {
                outcome: HostedStartOutcome::Existing,
                reason: None,
                session: Some(self.hosted_view(&checkout, session, live.as_deref(), false, tx)),
                role_prompt: None,
                tool_setup: None,
            });
        }
        // Nothing runs for this reservation yet, so a refusal forgets it and the same
        // request can be made again later.
        let refuse = |reason: &str| {
            registry.release(&checkout.id, &reserved.operation_id)?;
            Ok(HostedStartReply {
                outcome: HostedStartOutcome::Refused,
                reason: Some(reason.into()),
                session: None,
                role_prompt: None,
                tool_setup: None,
            })
        };
        if reserved.role.is_role_session() {
            let roles = Self::hosted_roles(&checkout, tx.clock.now_ms());
            let taken = match reserved.role {
                SessionRole::Integrator => roles.integrator_live,
                SessionRole::Dispatcher => roles.dispatcher_live,
                // HS2-HCW7MD: only this machine's own records hold a planner.
                SessionRole::Worker | SessionRole::Planner => Some(false),
            };
            let ours = registry.list(&checkout.id)?.into_iter().any(|other| {
                other.role == reserved.role
                    && other.started
                    && other.ended_at_ms.is_none()
                    && other.operation_id != reserved.operation_id
                    && live.as_ref().is_none_or(|live| {
                        live.iter()
                            .any(|state| state.session_id == other.session_id)
                    })
            });
            if taken == Some(true) || ours {
                return refuse(SessionError::RoleActive.code());
            }
        }
        let decision = match self.hosted_admission(
            checkout_id,
            AdmissionRequest {
                tool: reserved.tool.clone(),
                model: reserved.model.clone(),
                sessions: 1,
                limits: request.limits,
            },
        ) {
            Ok(decision) => decision,
            Err(error) => {
                registry.release(&checkout.id, &reserved.operation_id)?;
                return Err(error);
            }
        };
        if decision.outcome == AdmissionOutcome::Refused || decision.admitted == 0 {
            return refuse(
                decision
                    .reason
                    .map_or(SessionError::AdmissionRefused.code(), |reason| {
                        reason.code()
                    }),
            );
        }
        // The capacity reading is shared for a few seconds, so a burst of starts could
        // all see the same count. The cap is checked again on a fresh listing.
        let Some(live) = live else {
            return refuse(SessionError::HostUnavailable.code());
        };
        if live.len() >= usize::try_from(decision.limits.max_sessions).unwrap_or(usize::MAX)
            && !live
                .iter()
                .any(|state| state.session_id == reserved.session_id)
        {
            return refuse("session_cap_reached");
        }
        // HS2-WEFJV8: the tool starts in the target checkout, so that is where its MCP,
        // instruction and hook configuration has to be. The launch path sets up the
        // serving store only.
        let tool_setup = match self.sessions.prepare(&checkout, &reserved.tool) {
            Ok(setup) => setup,
            Err(SessionHostError::Unavailable) => {
                return refuse(SessionError::HostUnavailable.code());
            }
            Err(SessionHostError::Refused) => return refuse(TOOL_SETUP_FAILED),
        };
        let role_text = Self::role_text(&checkout, &reserved);
        let launched = match self.sessions.start(
            &checkout,
            &HostLaunch {
                session_id: reserved.session_id.clone(),
                tool: reserved.tool.clone(),
                model: reserved.model.clone(),
                effort: reserved.effort.clone(),
                label: reserved.label(),
                role: reserved.role,
                role_prompt: role_text.clone(),
            },
        ) {
            Ok(launched) => launched,
            Err(SessionHostError::Unavailable) => {
                return refuse(SessionError::HostUnavailable.code());
            }
            Err(SessionHostError::Refused) => return refuse(SessionError::StartFailed.code()),
        };
        if registry
            .mark_started(&checkout.id, &reserved.operation_id)?
            .is_none()
        {
            // HS2-WHR4GS: a stop withdrew the reservation while the tool was launching.
            // The stop was told nothing runs, so the terminal just launched is stopped
            // again rather than left running untracked.
            let _ = self.sessions.stop(&checkout, &reserved.session_id);
            return Ok(withdrawn());
        }
        // HS2-R0HSNK: a new terminal runs here. Every server on this machine announces
        // it (`terminal_orchestrator`), so drawers list it without waiting for the tool
        // to connect. A repeated start (`Existing`) or a refusal launched nothing and
        // announces nothing.
        self.hosted_session_signal();
        // HS2-2DPGDY: the prompt went with the launch, or it is pushed now; a tool
        // that is not ready yet is tried again on its readiness events.
        let attempt = (!launched.prompt_at_launch).then(|| {
            self.sessions
                .join_role(&checkout, &reserved.session_id, &role_text)
        });
        let started = registry
            .role_attempt(
                &checkout.id,
                &reserved.operation_id,
                attempt,
                tx.clock.now_ms(),
            )?
            .unwrap_or(reserved);
        let role_prompt = started
            .role_delivery
            .at(started.created_at_ms, tx.clock.now_ms());
        let live = self.sessions.sessions(&checkout);
        Ok(HostedStartReply {
            outcome: HostedStartOutcome::Started,
            reason: None,
            session: Some(self.hosted_view(&checkout, started, live.as_deref(), false, tx)),
            role_prompt,
            tool_setup: Some(tool_setup),
        })
    }

    /// HS2-2DPGDY: the prompt that asks this hosted session to take up its role.
    /// HS2-YC2P7K: a session started for a run asks for that run's exact ticket.
    fn role_text(checkout: &Checkout, session: &HostedSession) -> String {
        let worker = hotsheet_aitools::session_worker_id(&session.tool, &session.session_id);
        let request = session.origin.ticket.as_deref().map(|native| {
            Self::ticket_label(checkout, native).unwrap_or_else(|| native.to_owned())
        });
        // HS2-HCW7MD: a planner's ticket is the one goal it was asked to plan.
        if session.role == SessionRole::Planner {
            return planner_prompt_text(
                &checkout.id,
                &worker,
                &session.operation_id,
                request.as_deref(),
            );
        }
        let mut text = role_prompt_text(session.role, &worker, request.as_deref());
        // HS2-7YSKXE: the work's resources, for the session to acquire itself.
        if let Some(resources) = resources_prompt_text(&session.resources) {
            text.push(' ');
            text.push_str(&resources);
        }
        text
    }

    /// The slug of a ticket in the checkout's git stores, by its canonical id.
    fn ticket_label(checkout: &Checkout, native_id: &str) -> Option<String> {
        checkout
            .sources
            .iter()
            .filter(|source| source.provider == "git")
            .find_map(|source| {
                let store = checkout.verified_git_source(&source.connection_id).ok()?;
                ops::resolve(&store, native_id)
                    .ok()
                    .flatten()
                    .map(|ticket| ticket.slug)
            })
    }

    /// HS2-2DPGDY: the host learned something that may have made the session's tool
    /// ready (its push address was registered, or it finished a turn). A role prompt
    /// still pending is tried once more; anything else, or a terminal no orchestrator
    /// started, changes nothing. Never called on a timer.
    pub fn hosted_role_retry(
        &self,
        terminal_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<RolePrompt>, OrchestrationError> {
        let registry = self.hosted_sessions_registry();
        let Some(session) = registry.by_session(terminal_id)? else {
            return Ok(None);
        };
        let now_ms = tx.clock.now_ms();
        if session.ended_at_ms.is_some() {
            return Ok(session.role_delivery.at(session.created_at_ms, now_ms));
        }
        let role_due = session
            .role_delivery
            .should_retry(session.created_at_ms, now_ms);
        // HS2-R7QH35: a pending request is tried on the same events, after the role.
        let request_due = session.request.as_ref().is_some_and(|request| {
            request
                .delivery
                .should_retry(request.requested_at_ms, now_ms)
        });
        if !role_due && !request_due {
            return Ok(session.role_delivery.at(session.created_at_ms, now_ms));
        }
        let checkout = self.owner(&session.checkout_id)?;
        let mut current = session.clone();
        if role_due {
            let attempt = self.sessions.join_role(
                &checkout,
                &session.session_id,
                &Self::role_text(&checkout, &session),
            );
            if let Some(after) =
                registry.role_attempt(&checkout.id, &session.operation_id, Some(attempt), now_ms)?
            {
                current = after;
            }
        }
        if request_due {
            self.hosted_request_push(&checkout, &current, tx)?;
        }
        Ok(current.role_delivery.at(current.created_at_ms, now_ms))
    }

    /// HS2-R7QH35: push a hosted session's recorded request once and fold the answer
    /// into the record.
    fn hosted_request_push(
        &self,
        checkout: &Checkout,
        session: &HostedSession,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<RolePrompt>, OrchestrationError> {
        let Some(request) = &session.request else {
            return Ok(None);
        };
        let now_ms = tx.clock.now_ms();
        let worker = hotsheet_aitools::session_worker_id(&session.tool, &session.session_id);
        let label =
            Self::ticket_label(checkout, &request.ticket).unwrap_or_else(|| request.ticket.clone());
        let attempt = self.sessions.join_role(
            checkout,
            &session.session_id,
            &request_prompt_text(request.kind, &worker, &label, request.commit.as_deref()),
        );
        Ok(self
            .hosted_sessions_registry()
            .request_attempt(&checkout.id, &session.operation_id, attempt, now_ms)?
            .and_then(|after| {
                after
                    .request
                    .as_ref()
                    .and_then(|request| request.at(now_ms))
            }))
    }

    /// HS2-R7QH35: tell a running session this machine hosts which exact ticket it now
    /// works. The request is recorded and pushed once; a tool that is not ready gets
    /// it on its readiness events, like its role prompt. The same request again is
    /// not pushed twice. A person's session, or one that ended, is refused.
    pub fn hosted_session_request(
        &self,
        checkout_id: &str,
        request: HostedSessionRequest,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedRequestReply, OrchestrationError> {
        identifier(&request.operation_id, "bounded operation id required")?;
        identifier(&request.ticket, "bounded ticket id required")?;
        let checkout = self.owner(checkout_id)?;
        let registry = self.hosted_sessions_registry();
        let refused = |error: SessionError| HostedRequestReply {
            request_prompt: None,
            reason: Some(error.code().into()),
        };
        let Some(session) = registry.get(&checkout.id, &request.operation_id)? else {
            return Ok(refused(SessionError::NotFound));
        };
        if session.owner == SessionOwner::Human {
            return Ok(refused(SessionError::HumanOwned));
        }
        if !session.started || session.ended_at_ms.is_some() {
            return Ok(refused(SessionError::NotActive));
        }
        let now_ms = tx.clock.now_ms();
        let Some(recorded) = registry.request(
            &checkout.id,
            &request.operation_id,
            &request.ticket,
            request.kind,
            request.commit.as_deref(),
            now_ms,
        )?
        else {
            return Ok(refused(SessionError::NotFound));
        };
        let unasked = recorded
            .request
            .as_ref()
            .is_some_and(|request| request.delivery.is_unasked());
        let request_prompt = if unasked {
            self.hosted_request_push(&checkout, &recorded, tx)?
        } else {
            recorded
                .request
                .as_ref()
                .and_then(|request| request.at(now_ms))
        };
        Ok(HostedRequestReply {
            request_prompt,
            reason: None,
        })
    }

    /// Stop a hosted session. This machine checks the safe point itself: the
    /// orchestrator's word that a session is idle is never enough.
    /// [`Self::hosted_session_stop`] journals the answer (HS2-CRBR8X).
    pub(super) fn hosted_session_stop_unjournaled(
        &self,
        checkout_id: &str,
        request: HostedSessionStop,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedStopReply, OrchestrationError> {
        identifier(&request.operation_id, "bounded operation id required")?;
        let checkout = self.owner(checkout_id)?;
        let registry = self.hosted_sessions_registry();
        let refused = |reason: &str, safe_point, session| HostedStopReply {
            stopped: false,
            reason: Some(reason.into()),
            safe_point,
            session,
            fenced: false,
        };
        let human = is_human(request.actor.as_ref());
        let stop = StopRequest {
            force: request.force,
            human,
        };
        let recorded = registry.get(&checkout.id, &request.operation_id)?;
        let session = match recorded {
            Some(session) if session.started => session,
            unlaunched => {
                if let Some(session) = &unlaunched {
                    // Reserved and never launched: there is nothing to interrupt.
                    let nothing_runs = decide_safe_point(&SafePointFacts {
                        alive: Some(false),
                        ..SafePointFacts::default()
                    });
                    if let Err(error) = decide_stop(session.owner, stop, &nothing_runs) {
                        return Ok(refused(error.code(), None, None));
                    }
                }
                // HS2-WHR4GS: released or unknown, the operation is fenced, so a start
                // still on its way launches nothing. One that launched meanwhile is
                // stopped like any launched session.
                match registry.withdraw(&checkout.id, &request.operation_id, tx.clock.now_ms())? {
                    Withdrawal::Unknown => {
                        return Ok(HostedStopReply {
                            fenced: true,
                            ..refused(SessionError::NotFound.code(), None, None)
                        });
                    }
                    Withdrawal::Released => {
                        return Ok(HostedStopReply {
                            stopped: true,
                            reason: None,
                            safe_point: None,
                            session: None,
                            fenced: true,
                        });
                    }
                    Withdrawal::Launched(session) => *session,
                }
            }
        };
        let live = self.sessions.sessions(&checkout);
        let session = self.note_lost(session, live.as_deref(), tx.clock.now_ms())?;
        let view = self.hosted_view(&checkout, session.clone(), live.as_deref(), true, tx);
        if session.ended_at_ms.is_some() {
            return Ok(HostedStopReply {
                stopped: true,
                reason: session.end_reason.clone(),
                safe_point: None,
                session: Some(view),
                fenced: false,
            });
        }
        let safe_point = view.safe_point.clone().unwrap_or_else(|| {
            // `hosted_view` was asked for one; an absent answer is not a safe one.
            decide_safe_point(&SafePointFacts::default())
        });
        if let Err(error) = decide_stop(session.owner, stop, &safe_point) {
            return Ok(refused(error.code(), Some(safe_point), Some(view)));
        }
        match self.sessions.stop(&checkout, &session.session_id) {
            Ok(()) => {}
            Err(_) => {
                return Ok(refused(
                    SessionError::HostUnavailable.code(),
                    Some(safe_point),
                    Some(view),
                ));
            }
        }
        let ended = registry
            .end(
                &checkout.id,
                &request.operation_id,
                if human {
                    "person_stopped"
                } else {
                    "wound_down"
                },
                tx.clock.now_ms(),
            )?
            .unwrap_or(session);
        self.hosted_session_signal();
        Ok(HostedStopReply {
            stopped: true,
            reason: ended.end_reason.clone(),
            safe_point: Some(safe_point),
            session: Some(self.hosted_view(&checkout, ended, None, false, tx)),
            fenced: false,
        })
    }

    /// A person takes a hosted session over. From then on this machine refuses an
    /// orchestrator's stop for it.
    pub fn hosted_session_control(
        &self,
        checkout_id: &str,
        request: HostedSessionControl,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedControlReply, OrchestrationError> {
        identifier(&request.operation_id, "bounded operation id required")?;
        if !is_human(request.actor.as_ref()) {
            return Err(OrchestrationError::Forbidden(
                "only a person takes over or pauses an orchestrator-owned session",
            ));
        }
        let checkout = self.owner(checkout_id)?;
        let taken = self.hosted_sessions_registry().take_over(
            &checkout.id,
            &request.operation_id,
            request.pause,
        )?;
        if taken.is_some() {
            self.hosted_session_signal();
        }
        let live = self.sessions.sessions(&checkout);
        Ok(HostedControlReply {
            session: taken
                .map(|session| self.hosted_view(&checkout, session, live.as_deref(), false, tx)),
        })
    }

    // ---- Orchestrating half ----

    pub(super) fn session_registry(&self) -> SessionRegistry {
        SessionRegistry::new(&self.home)
    }

    pub fn session(
        &self,
        owner_id: &str,
        session_id: &str,
    ) -> Result<OrchestratedSession, OrchestrationError> {
        self.session_registry().get(owner_id, session_id)
    }

    /// This machine's records of the sessions the owner started. No target is asked.
    pub fn session_list(
        &self,
        owner_id: &str,
        active: bool,
    ) -> Result<Vec<OrchestratedSession>, OrchestrationError> {
        self.session_registry().list(owner_id, active)
    }

    /// The sessions already occupying the target, its machine, the tool and the
    /// account, as the policy's caps count them.
    pub fn session_load(
        &self,
        owner_id: &str,
        target_id: &str,
        tool: &str,
        account: Option<&str>,
    ) -> Result<SessionLoad, OrchestrationError> {
        let target = Self::target_ref(owner_id, target_id)?;
        let server = Self::binding_at(&self.home, &target)
            .ok()
            .and_then(|binding| binding.destination)
            .map(|destination| server_of(&destination).to_owned());
        self.session_registry()
            .load(owner_id, target_id, server.as_deref(), tool, account)
    }

    /// HS2-75Q254: the session source of a run from this machine's records alone,
    /// classified as reconciliation classifies it, without asking any host. Use
    /// `run_session_work` to sync with the hosts first.
    pub fn session_work(&self, owner_id: &str, run_id: &str) -> Observed<ExternalWork> {
        let Ok(run) = self.run(owner_id, run_id) else {
            return Observed::Verified(ExternalWork {
                note: Some(hotsheet_ticketing::orchestration::NOTE_NO_SESSION.into()),
                ..ExternalWork::default()
            });
        };
        match self.session_registry().list(owner_id, false) {
            Ok(sessions) => {
                let seen: Vec<_> = sessions
                    .into_iter()
                    .filter(|session| session.run_id.as_deref() == Some(run_id))
                    .map(|session| (session, None))
                    .collect();
                hotsheet_ticketing::orchestration::session_work(
                    &seen,
                    self.session_need(owner_id, &run, 0),
                )
            }
            Err(_) => Observed::Unverifiable("session_records_unreadable".into()),
        }
    }

    /// Apply a change to a session and publish what changed for its run. Every session
    /// update goes through here, so the stream and the record cannot drift apart.
    fn session_apply(
        &self,
        owner_id: &str,
        session_id: &str,
        update: SessionUpdate,
        from_target: bool,
        tx: &mut Transaction<'_>,
    ) -> Result<OrchestratedSession, OrchestrationError> {
        let registry = self.session_registry();
        let before = registry.get(owner_id, session_id)?;
        let actor_id = update.actor_id.clone();
        let after = registry.update(owner_id, session_id, update, tx)?;
        let changed = after.history.len() != before.history.len()
            || before.history.last() != after.history.last();
        // HS2-2DPGDY: joining and a new role answer are news without a state change.
        let joined = before.joined_at_ms.is_none() && after.joined_at_ms.is_some();
        let prompted = before.role_prompt != after.role_prompt && after.role_prompt.is_some();
        let assigned = before.run_id != after.run_id;
        if (joined || prompted) && after.run_id.is_none() {
            Self::scheduler_wake_at(&self.home);
        }
        if changed && after.run_id.is_none() {
            // HS2-95SWQ4: a session without a run has no event stream to publish on,
            // so its start, exit and takeover wake the scheduler through its marker.
            Self::scheduler_wake_at(&self.home);
        }
        if let (Some(run_id), Some(entry)) = (&after.run_id, after.history.last()) {
            if changed {
                self.run_event(
                    owner_id,
                    RunEvent {
                        at_ms: after.updated_at_ms,
                        run_id: Some(run_id.clone()),
                        step_id: None,
                        kind: RunEventKind::Session,
                        source: if from_target {
                            RunEventSource::Target
                        } else {
                            RunEventSource::Orchestrator
                        },
                        state: None,
                        // A control act is published as what it was, a plain change
                        // as the state the session entered.
                        reason: Some(match entry.control {
                            Some(control) if entry.from == Some(entry.to) => {
                                format!("session_{}", control.code())
                            }
                            _ => format!("session_{}", entry.to.code()),
                        }),
                        actor_id: actor_id.clone(),
                        dedupe: Some(format!(
                            "session-{}-{}-{}",
                            after.session_id,
                            after.history.len(),
                            after.updated_at_ms
                        )),
                        gate_id: None,
                        gate_state: None,
                    },
                );
            }
        }
        if let Some(run_id) = after
            .run_id
            .as_ref()
            .filter(|_| joined || prompted || assigned)
        {
            let news = [
                // HS2-YC2P7K: the scheduler gave a running session this run.
                (before.run_id != after.run_id).then(|| "session_assigned".to_owned()),
                joined.then(|| "session_joined".to_owned()),
                after
                    .role_prompt
                    .filter(|_| prompted)
                    .map(|prompt| format!("session_role_{}", prompt.code())),
            ];
            for reason in news.into_iter().flatten() {
                self.run_event(
                    owner_id,
                    RunEvent {
                        at_ms: after.updated_at_ms,
                        run_id: Some(run_id.clone()),
                        step_id: None,
                        kind: RunEventKind::Session,
                        source: if from_target {
                            RunEventSource::Target
                        } else {
                            RunEventSource::Orchestrator
                        },
                        state: None,
                        dedupe: Some(format!("{}-{reason}", after.session_id)),
                        reason: Some(reason),
                        actor_id: actor_id.clone(),
                        gate_id: None,
                        gate_state: None,
                    },
                );
            }
        }
        Ok(after)
    }

    /// HS2-YC2P7K: link a running session to the run the scheduler gave it. Published
    /// as `session_assigned` on that run.
    pub(super) fn session_link(
        &self,
        owner_id: &str,
        session_id: &str,
        run_id: &str,
        actor: &MutationActor,
        tx: &mut Transaction<'_>,
    ) -> Result<OrchestratedSession, OrchestrationError> {
        let session = self.session(owner_id, session_id)?;
        if self.run(owner_id, run_id)?.target.target_id != session.target_id {
            return Err(OrchestrationError::Invalid(
                "the run belongs to another target",
            ));
        }
        self.session_apply(
            owner_id,
            session_id,
            SessionUpdate {
                run_id: Some(run_id.to_owned()),
                actor_id: actor.id.clone(),
                ..SessionUpdate::default()
            },
            false,
            tx,
        )
    }

    fn session_request(
        session: &OrchestratedSession,
        starting: bool,
        running: u32,
    ) -> SessionRequest {
        SessionRequest {
            tool: session.tool.clone(),
            model: session.model.clone(),
            running,
            starting,
        }
    }

    /// The gate for reading or winding down an existing session: it only reduces
    /// activity, so it never asks and the policy never denies it (HS2-48H57C),
    /// nothing new starts, and it carries no load. It is passed under
    /// the session's own operation id, or under the scheduler's for a stop it placed.
    pub(super) fn session_gate(
        &self,
        session: &OrchestratedSession,
        actor: Option<&MutationActor>,
        continuing: bool,
        placed: Option<&PlacedGate<'_>>,
        tx: &mut Transaction<'_>,
    ) -> Result<(TargetRef, Authorized), OrchestrationError> {
        let target = Self::target_ref(&session.owner_checkout_id, &session.target_id)?;
        let request = ActionRequest {
            session: Some(Self::session_request(session, false, 0)),
            role_session: session.role.is_role_session(),
            ..ActionRequest::new(TargetAction::SessionsManage).operation(
                placed.map_or(session.operation_id.as_str(), |placed| placed.operation_id),
            )
        };
        let authorized = self.authorize(
            &target,
            actor,
            if continuing {
                request.continuing()
            } else {
                request
            },
            tx,
        )?;
        Ok((target, authorized))
    }

    /// A person needs no permission request to act on a session of their own
    /// orchestration project. The target is resolved freshly and the act is audited here.
    fn person_destination(
        &self,
        target: &TargetRef,
        tx: &mut Transaction<'_>,
    ) -> Result<TargetDestination, OrchestrationError> {
        let resolution = self.resolve(target, tx)?;
        let usable = resolution.usable;
        resolution
            .binding
            .destination
            .filter(|_| usable)
            .ok_or(OrchestrationError::Denied(DenialReason::TargetUnavailable))
    }

    fn session_audit(
        &self,
        session: &OrchestratedSession,
        actor: Option<&MutationActor>,
        action: &'static str,
        outcome: &Result<SessionView, OrchestrationError>,
        tx: &mut Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        self.audit_record(
            &session.owner_checkout_id,
            AuditRecord {
                at_ms: tx.clock.now_ms(),
                actor_role: super::access::role_code(actor),
                actor_id: actor.and_then(|actor| actor.id.clone()),
                target_id: Some(session.target_id.clone()),
                action: action.into(),
                operation_id: Some(session.operation_id.clone()),
                outcome: if outcome.is_ok() {
                    AuditOutcome::Succeeded
                } else {
                    AuditOutcome::Failed
                },
                reason: outcome.as_ref().err().map(|error| error.code().into()),
                permission_id: None,
                requested_by: None,
            },
        )
    }

    fn hosted_listing(
        &self,
        destination: &TargetDestination,
        session: &OrchestratedSession,
        with_safe_point: bool,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<HostedSessionView>, OrchestrationError> {
        let query = format!(
            "?operation_id={}{}",
            encode(&session.operation_id),
            if with_safe_point {
                "&safe_point=true"
            } else {
                ""
            }
        );
        let list: HostedSessionList = serde_json::from_value(self.hosted_call_at(
            destination,
            "/target-sessions",
            "GET",
            &query,
            None,
            tx,
        )?)
        .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))?;
        Ok(list
            .sessions
            .into_iter()
            .find(|hosted| hosted.session.operation_id == session.operation_id))
    }

    /// Bring this machine's record in line with what the host answered.
    fn session_observe(
        &self,
        session: &OrchestratedSession,
        hosted: Option<&HostedSessionView>,
        tx: &mut Transaction<'_>,
    ) -> Result<OrchestratedSession, OrchestrationError> {
        match self.session_observe_once(session, hosted, tx) {
            // HS2-3REXQ7: a concurrent reader (the scheduler's loop, a follower) ended
            // the session between this read of the record and its update. That reader
            // saw the same host; its record is the answer.
            Err(OrchestrationError::Session(SessionError::NotActive)) => {
                let current = self.session(&session.owner_checkout_id, &session.session_id)?;
                if current.state.is_terminal() {
                    Ok(current)
                } else {
                    Err(OrchestrationError::Session(SessionError::NotActive))
                }
            }
            other => other,
        }
    }

    fn session_observe_once(
        &self,
        session: &OrchestratedSession,
        hosted: Option<&HostedSessionView>,
        tx: &mut Transaction<'_>,
    ) -> Result<OrchestratedSession, OrchestrationError> {
        if session.state.is_terminal() {
            return Ok(session.clone());
        }
        let owner_id = &session.owner_checkout_id;
        let mut update = SessionUpdate::default();
        match hosted {
            // The host has no record. A session it once confirmed is lost; one it never
            // confirmed may still be asked for again under the same operation id.
            None if session.state == SessionState::Running => {
                update.to = Some(SessionState::Lost);
                update.reason = Some("host_forgot".into());
            }
            None => {}
            Some(hosted) => {
                update.hosted = Some(HostedRef {
                    server_id: hosted.server_id.clone(),
                    session_id: hosted.session.session_id.clone(),
                    worker_id: hosted.worker_id.clone(),
                });
                // HS2-2DPGDY: the role as its host answers now, and joining as the
                // target's pool shows it.
                update.role_prompt = hosted.role_prompt;
                update.joined_at_ms = (hosted.joined == Some(true)).then(|| tx.clock.now_ms());
                if hosted.session.owner == SessionOwner::Human {
                    // Learned from the host: a person took it over there.
                    update.owner = Some(SessionOwner::Human);
                    update.paused = Some(hosted.session.paused);
                    update.control = Some(if hosted.session.paused {
                        SessionControl::Pause
                    } else {
                        SessionControl::Takeover
                    });
                    update.reason = Some("host_takeover".into());
                }
                let ended = hosted.session.end_reason.as_deref();
                match (session.state, hosted.alive, ended) {
                    (_, _, Some(reason @ ("person_stopped" | "wound_down"))) => {
                        if session.state == SessionState::Requested {
                            // It ran and was stopped before this record learned it ran.
                            self.session_apply(
                                owner_id,
                                &session.session_id,
                                SessionUpdate {
                                    to: Some(SessionState::Running),
                                    reason: Some("started".into()),
                                    hosted: update.hosted.clone(),
                                    ..SessionUpdate::default()
                                },
                                true,
                                tx,
                            )?;
                        }
                        update.to = Some(SessionState::Stopped);
                        update.reason = Some(reason.into());
                        update.control = None;
                    }
                    (SessionState::Requested, Some(false), _) if hosted.session.started => {
                        self.session_apply(
                            owner_id,
                            &session.session_id,
                            SessionUpdate {
                                to: Some(SessionState::Running),
                                reason: Some("started".into()),
                                hosted: update.hosted.clone(),
                                ..SessionUpdate::default()
                            },
                            true,
                            tx,
                        )?;
                        update.to = Some(SessionState::Lost);
                        update.reason = Some("host_lost".into());
                    }
                    (SessionState::Running, Some(false), _) => {
                        update.to = Some(SessionState::Lost);
                        update.reason = Some("host_lost".into());
                    }
                    (SessionState::Requested, _, _) if hosted.session.started => {
                        update.to = Some(SessionState::Running);
                        update.reason = Some("started".into());
                    }
                    _ => {}
                }
            }
        }
        // An ownership change and a state change learned together are two facts.
        if update.to.is_some() && update.owner.is_some() {
            self.session_apply(
                owner_id,
                &session.session_id,
                SessionUpdate {
                    to: None,
                    ..update.clone()
                },
                true,
                tx,
            )?;
            update.owner = None;
            update.paused = None;
            update.control = None;
        }
        self.session_apply(owner_id, &session.session_id, update, true, tx)
    }

    /// Start a session on the target's own server. Repeating the request with the same
    /// operation id returns the same session and never launches twice.
    pub fn session_start(
        &self,
        owner_id: &str,
        request: StartSession,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<SessionView, OrchestrationError> {
        self.session_start_gated(owner_id, request, None, actor, tx)
    }

    /// [`Self::session_start`], with how the gate sees a start the scheduler placed.
    pub(super) fn session_start_gated(
        &self,
        owner_id: &str,
        request: StartSession,
        placed: Option<&PlacedGate<'_>>,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<SessionView, OrchestrationError> {
        identifier(&request.operation_id, "bounded operation id required")?;
        identifier(&request.tool, "bounded tool id required")?;
        let target = Self::target_ref(owner_id, &request.target_id)?;
        let owner = self.owner(owner_id)?;
        let role = request.role.unwrap_or_default();
        let mut run_ticket = None;
        if let Some(run_id) = &request.run_id {
            // A session is only ever attached to a run of the same target.
            let run = self.run(owner_id, run_id)?;
            if run.target.target_id != request.target_id {
                return Err(OrchestrationError::Invalid(
                    "the run belongs to another target",
                ));
            }
            // HS2-YC2P7K: its role prompt asks for the run's exact ticket.
            run_ticket = Some(run.target.native_id);
        }
        let new = NewSession {
            operation_id: request.operation_id.clone(),
            target_id: request.target_id.clone(),
            role,
            tool: request.tool.clone(),
            model: request.model.clone(),
            effort: request.effort.clone(),
            account: request.account.clone(),
            run_id: request.run_id.clone(),
            replaces: request.replaces.clone(),
        };
        let registry = self.session_registry();
        // HS2-DG0YPA: another live session of the run is asked about first, so a holder
        // that crashed is `lost` and no longer holds the work. One its host cannot be
        // asked about stays as it is, and the start is refused rather than doubled.
        if let Some(run_id) = &request.run_id {
            self.sync_run_holders(owner_id, run_id, &request.operation_id, actor, tx);
        }
        // The registry decides whether this is a repeat, a conflict or a start that can
        // never be recorded, before a person or the target is asked anything.
        let known = registry.check(owner_id, &new)?;
        if let Some(known) = known
            .as_ref()
            .filter(|known| known.state != SessionState::Requested)
        {
            return Ok(SessionView::from(known.clone()));
        }
        // A start still in `requested` is being retried: it does not count against
        // its own caps.
        let mut load = self.session_load(
            owner_id,
            &request.target_id,
            &request.tool,
            request.account.as_deref(),
        )?;
        if known.is_some() {
            load.target_sessions = load.target_sessions.saturating_sub(1);
            load.machine_sessions = load.machine_sessions.saturating_sub(1);
            load.tool_sessions = load.tool_sessions.saturating_sub(1);
            load.account_sessions = load.account_sessions.saturating_sub(1);
        }
        if let Some(decided) = placed.and_then(|placed| placed.load) {
            // HS2-95SWQ4: the scheduler decided on a count that includes sessions these
            // records do not hold (other owners' and people's on the machine), and
            // the records hold starts made since it counted. Neither is undercut.
            load.target_sessions = load.target_sessions.max(decided.target_sessions);
            load.machine_sessions = load.machine_sessions.max(decided.machine_sessions);
            load.tool_sessions = load.tool_sessions.max(decided.tool_sessions);
            if load.account == decided.account {
                load.account_sessions = load.account_sessions.max(decided.account_sessions);
            }
        }
        let authorized = self.authorize(
            &target,
            actor,
            ActionRequest {
                session: Some(SessionRequest {
                    tool: request.tool.clone(),
                    model: request.model.clone(),
                    running: load.target_sessions,
                    starting: true,
                }),
                load: Some(load),
                role_session: role.is_role_session(),
                ..ActionRequest::new(TargetAction::SessionsManage).operation(
                    placed.map_or(request.operation_id.as_str(), |placed| placed.operation_id),
                )
            },
            tx,
        )?;
        let outcome = (|| {
            let destination = Self::destination(&authorized)?;
            let (session, _) =
                registry.begin(owner_id, new, actor.and_then(|actor| actor.id.clone()), tx)?;
            if session.state != SessionState::Requested {
                return Ok(SessionView::from(session));
            }
            // The gate just evaluated this policy; one that vanished since is a conflict.
            let policy = self
                .policy(owner_id)?
                .policy
                .ok_or(OrchestrationError::Conflict)?;
            let body = serde_json::to_value(HostedSessionStart {
                session: NewHostedSession {
                    account: session.account.clone(),
                    operation_id: session.operation_id.clone(),
                    tool: session.tool.clone(),
                    model: session.model.clone(),
                    effort: session.effort.clone(),
                    role,
                    origin: SessionOrigin {
                        owner_checkout_id: owner.id.clone(),
                        project: project_name(&owner),
                        run_id: session.run_id.clone(),
                        ticket: run_ticket.clone(),
                    },
                },
                limits: Some(policy.machine_limits(&request.target_id)),
                actor: actor.cloned(),
                resources: request.resources.clone(),
            })
            .map_err(|_| OrchestrationError::Corrupt)?;
            let fail = |reason: &str, tx: &mut Transaction<'_>| {
                self.session_apply(
                    owner_id,
                    &session.session_id,
                    SessionUpdate {
                        to: Some(SessionState::Failed),
                        reason: Some(reason.into()),
                        actor_id: actor.and_then(|actor| actor.id.clone()),
                        ..SessionUpdate::default()
                    },
                    false,
                    tx,
                )
            };
            let reply = match self.hosted_call_at(
                destination,
                "/target-sessions",
                "POST",
                "",
                Some(&body),
                tx,
            ) {
                Ok(reply) => serde_json::from_value::<HostedStartReply>(reply)
                    .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))?,
                // The target said no for good: the session ends, and the same
                // operation id keeps answering with that failed session.
                Err(OrchestrationError::TargetOperation(
                    error @ (TargetOpError::Conflict
                    | TargetOpError::Unsupported
                    | TargetOpError::NotFound
                    | TargetOpError::Invalid
                    | TargetOpError::Disabled
                    | TargetOpError::PeerUnsupported),
                )) => {
                    fail(error.code(), tx)?;
                    return Err(OrchestrationError::TargetOperation(error));
                }
                // Anything else may be transient: the session stays `requested` so the
                // same request can be repeated.
                Err(error) => return Err(error),
            };
            let Some(hosted) = reply
                .session
                .filter(|_| reply.outcome != HostedStartOutcome::Refused)
            else {
                let reason = reply
                    .reason
                    .filter(|reason| identifier(reason, "code").is_ok())
                    .unwrap_or_else(|| SessionError::AdmissionRefused.code().into());
                fail(&reason, tx)?;
                // A session code is reported as itself; an admission reason as a
                // refused admission, with the reason in the session's history.
                return Err(OrchestrationError::Session(
                    SessionError::from_code(&reason).unwrap_or(if reason == TOOL_SETUP_FAILED {
                        // The machine had room; the start itself could not be made.
                        SessionError::StartFailed
                    } else {
                        SessionError::AdmissionRefused
                    }),
                ));
            };
            if hosted.session.ended_at_ms.is_some() {
                // The host launched it for an earlier request that never heard back,
                // and it has ended since: record both facts, not a running session.
                let after = self.session_observe(&session, Some(&hosted), tx)?;
                return Ok(SessionView {
                    label: Some(hosted.label),
                    ..SessionView::from(after)
                });
            }
            let started = self.session_apply(
                owner_id,
                &session.session_id,
                SessionUpdate {
                    to: Some(SessionState::Running),
                    reason: Some("started".into()),
                    actor_id: actor.and_then(|actor| actor.id.clone()),
                    hosted: Some(HostedRef {
                        server_id: hosted.server_id.clone(),
                        session_id: hosted.session.session_id.clone(),
                        worker_id: hosted.worker_id.clone(),
                    }),
                    role_prompt: reply.role_prompt,
                    ..SessionUpdate::default()
                },
                false,
                tx,
            )?;
            Ok(SessionView {
                label: Some(hosted.label),
                role_prompt: reply.role_prompt,
                tool_setup: reply.tool_setup,
                ..SessionView::from(started)
            })
        })();
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }

    /// HS2-DG0YPA: bring every live session of a run other than the one `operation_id`
    /// names in line with its host, as `actor`. Best effort: a session that cannot be
    /// read keeps its state, and so keeps holding the run.
    pub(super) fn sync_run_holders(
        &self,
        owner_id: &str,
        run_id: &str,
        operation_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) {
        let Ok(sessions) = self.session_list(owner_id, true) else {
            return;
        };
        for session in sessions.iter().filter(|session| {
            session.run_id.as_deref() == Some(run_id) && session.operation_id != operation_id
        }) {
            let _ = self.session_sync(owner_id, &session.session_id, actor, tx);
        }
    }

    /// HS2-R7QH35: tell a running session which exact ticket its run is for, through
    /// its hosting server's session push with the same readiness-aware delivery as its
    /// role prompt. `claimed` when its run's ticket is claimed under its own worker id
    /// (HS2-T16SK0), otherwise `assigned`: request it from the dispatcher. Passes the
    /// gate as `sessions_manage` continuing, and publishes `session_request_<code>` on
    /// the run's stream. A person's session is never sent anything.
    pub fn session_tell_ticket(
        &self,
        owner_id: &str,
        session_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<SessionView, OrchestrationError> {
        self.session_tell(owner_id, session_id, None, actor, tx)
    }

    /// HS2-2K0DM8: tell the session working at-risk provisional work that the base
    /// gate's fix is `commit`: rebase onto it and rerun the affected gates. Like every
    /// tell it passes the gate as `sessions_manage` continuing: it supervises a run
    /// already started, asks nobody, and a person's session is never sent anything.
    pub fn session_tell_rebase(
        &self,
        owner_id: &str,
        session_id: &str,
        commit: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<SessionView, OrchestrationError> {
        self.session_tell(
            owner_id,
            session_id,
            Some((RequestKind::Rebase, Some(commit.to_owned()))),
            actor,
            tx,
        )
    }

    /// [`Self::session_tell_ticket`], or with `RequestKind::Cancelled` the message
    /// that a person cancelled the session's run (HS2-HV9ED3), which is finished.
    pub(super) fn session_tell(
        &self,
        owner_id: &str,
        session_id: &str,
        told: Option<(RequestKind, Option<String>)>,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<SessionView, OrchestrationError> {
        let (cancelled, commit) = match told {
            Some((kind, commit)) => (Some(kind), commit),
            None => (None, None),
        };
        let session = self.session(owner_id, session_id)?;
        if session.state != SessionState::Running {
            return Err(OrchestrationError::Session(SessionError::NotActive));
        }
        if session.owner == SessionOwner::Human {
            return Err(OrchestrationError::Session(SessionError::HumanOwned));
        }
        let run_id = session
            .run_id
            .clone()
            .ok_or(OrchestrationError::Invalid("the session works no run"))?;
        let run = self.run(owner_id, &run_id)?;
        let told_cancel = cancelled == Some(RequestKind::Cancelled)
            && run.state == hotsheet_ticketing::orchestration::RunState::Cancelled;
        if run.state.is_terminal() && !told_cancel {
            return Err(OrchestrationError::Run(
                hotsheet_ticketing::orchestration::RunError::NotActive,
            ));
        }
        let worker = session
            .hosted
            .as_ref()
            .map(|hosted| hosted.worker_id.as_str());
        let kind = if told_cancel {
            RequestKind::Cancelled
        } else if cancelled == Some(RequestKind::Rebase) {
            RequestKind::Rebase
        } else if run.state == hotsheet_ticketing::orchestration::RunState::Dispatched
            && run.worker_id.as_deref() == worker
        {
            RequestKind::Claimed
        } else {
            RequestKind::Assigned
        };
        let (target, authorized) = self.session_gate(&session, actor, true, None, tx)?;
        let outcome = (|| {
            let body = serde_json::to_value(HostedSessionRequest {
                operation_id: session.operation_id.clone(),
                ticket: run.target.native_id.clone(),
                kind,
                commit: commit.clone().filter(|_| kind == RequestKind::Rebase),
                actor: actor.cloned(),
            })
            .map_err(|_| OrchestrationError::Corrupt)?;
            let reply: HostedRequestReply = serde_json::from_value(self.hosted_call_at(
                Self::destination(&authorized)?,
                "/target-sessions",
                "PUT",
                "",
                Some(&body),
                tx,
            )?)
            .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))?;
            if let Some(reason) = reply.reason {
                return Err(SessionError::from_code(&reason).map_or(
                    OrchestrationError::Observation(TargetFailure::InvalidResponse),
                    OrchestrationError::Session,
                ));
            }
            if let Some(prompt) = reply.request_prompt {
                let reason = format!("session_request_{}", prompt.code());
                self.run_event(
                    owner_id,
                    RunEvent {
                        at_ms: tx.clock.now_ms(),
                        run_id: Some(run_id.clone()),
                        step_id: None,
                        kind: RunEventKind::Session,
                        source: RunEventSource::Orchestrator,
                        state: None,
                        dedupe: Some(format!(
                            "{session_id}-{run_id}-{}-{reason}{}",
                            kind.code(),
                            commit
                                .as_deref()
                                .map(|commit| format!("-{commit}"))
                                .unwrap_or_default()
                        )),
                        reason: Some(reason),
                        actor_id: actor.and_then(|actor| actor.id.clone()),
                        gate_id: None,
                        gate_state: None,
                    },
                );
            }
            Ok(SessionView {
                request_prompt: reply.request_prompt,
                ..SessionView::from(session.clone())
            })
        })();
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }

    /// Read the session from its host and bring the record up to date: a session the
    /// host lost becomes `lost`, one a person took over there becomes theirs.
    pub fn session_sync(
        &self,
        owner_id: &str,
        session_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<SessionView, OrchestrationError> {
        self.session_read(owner_id, session_id, false, actor, tx)
    }

    /// Whether the session may be wound down now, as its own host established it.
    pub fn session_safe_point(
        &self,
        owner_id: &str,
        session_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<SessionView, OrchestrationError> {
        self.session_read(owner_id, session_id, true, actor, tx)
    }

    fn session_read(
        &self,
        owner_id: &str,
        session_id: &str,
        with_safe_point: bool,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<SessionView, OrchestrationError> {
        let session = self.session(owner_id, session_id)?;
        if session.state.is_terminal() {
            // Nothing runs, so there is nothing to interrupt and nobody to ask.
            return Ok(SessionView {
                safe_point: with_safe_point.then(|| {
                    decide_safe_point(&SafePointFacts {
                        alive: Some(false),
                        busy: Some(false),
                        ..SafePointFacts::default()
                    })
                }),
                ..SessionView::from(session)
            });
        }
        let (target, authorized) = self.session_gate(&session, actor, true, None, tx)?;
        let outcome = (|| {
            let hosted = self.hosted_listing(
                Self::destination(&authorized)?,
                &session,
                with_safe_point,
                tx,
            )?;
            let after = self.session_observe(&session, hosted.as_ref(), tx)?;
            Ok(SessionView {
                label: hosted.as_ref().map(|hosted| hosted.label.clone()),
                safe_point: match &hosted {
                    Some(hosted) => hosted.safe_point.clone(),
                    None => with_safe_point.then(|| {
                        decide_safe_point(&SafePointFacts {
                            alive: Some(false),
                            busy: Some(false),
                            ..SafePointFacts::default()
                        })
                    }),
                },
                ..SessionView::from(after)
            })
        })();
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }

    /// Sync every unfinished session of the owner. One that cannot be examined is
    /// listed as failed with its code and left as it was.
    pub fn sessions_sync(
        &self,
        owner_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<SessionSyncSummary, OrchestrationError> {
        let mut summary = SessionSyncSummary {
            sessions: Vec::new(),
            failed: Vec::new(),
        };
        for session in self.session_list(owner_id, true)? {
            match self.session_sync(owner_id, &session.session_id, actor, tx) {
                Ok(view) => summary.sessions.push(view),
                Err(error) => summary.failed.push(SessionSyncFailure {
                    session_id: session.session_id,
                    code: error.code(),
                }),
            }
        }
        Ok(summary)
    }

    /// Stop a session. The orchestrator winds down only its own sessions and only when
    /// the host confirms a safe point; `force` is a person's explicit request.
    pub fn session_stop(
        &self,
        owner_id: &str,
        session_id: &str,
        request: StopSession,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<SessionView, OrchestrationError> {
        self.session_stop_gated(owner_id, session_id, request, None, actor, tx)
    }

    /// [`Self::session_stop`], with how the gate sees a stop the scheduler placed.
    pub(super) fn session_stop_gated(
        &self,
        owner_id: &str,
        session_id: &str,
        request: StopSession,
        placed: Option<&PlacedGate<'_>>,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<SessionView, OrchestrationError> {
        let session = self.session(owner_id, session_id)?;
        if session.state.is_terminal() {
            return Err(OrchestrationError::Session(SessionError::NotActive));
        }
        let human = is_human(actor);
        let actor_id =
            actor
                .and_then(|actor| actor.id.clone())
                .ok_or(OrchestrationError::Invalid(
                    "an identified actor is required to stop a session",
                ))?;
        // The cheap refusals first: nothing is asked of the target for them.
        if request.force && !human {
            return Err(OrchestrationError::Session(SessionError::ForceNotPermitted));
        }
        if session.owner == SessionOwner::Human && !human {
            return Err(OrchestrationError::Session(SessionError::HumanOwned));
        }
        let target = Self::target_ref(owner_id, &session.target_id)?;
        let (destination, authorized) = if human {
            (self.person_destination(&target, tx)?, None)
        } else {
            let (_, authorized) = self.session_gate(&session, actor, false, placed, tx)?;
            (Self::destination(&authorized)?.clone(), Some(authorized))
        };
        let outcome = (|| {
            let body = serde_json::to_value(HostedSessionStop {
                operation_id: session.operation_id.clone(),
                force: request.force,
                actor: actor.cloned(),
            })
            .map_err(|_| OrchestrationError::Corrupt)?;
            let reply: HostedStopReply = serde_json::from_value(self.hosted_call_at(
                &destination,
                "/target-sessions",
                "DELETE",
                "",
                Some(&body),
                tx,
            )?)
            .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))?;
            let stopped = |reason: &str, tx: &mut Transaction<'_>| {
                self.session_apply(
                    owner_id,
                    session_id,
                    SessionUpdate {
                        to: Some(SessionState::Stopped),
                        reason: Some(reason.into()),
                        actor_id: Some(actor_id.clone()),
                        control: Some(if human {
                            SessionControl::Stop
                        } else {
                            SessionControl::WindDown
                        }),
                        ..SessionUpdate::default()
                    },
                    false,
                    tx,
                )
            };
            if !reply.stopped {
                return match reply.reason.as_deref().and_then(SessionError::from_code) {
                    // The host never had it, so nothing runs for a start that was
                    // never confirmed. A session it once confirmed is lost.
                    Some(SessionError::NotFound) if session.state == SessionState::Requested => {
                        stopped("never_started", tx).map(SessionView::from)
                    }
                    Some(SessionError::NotFound) => self
                        .session_observe(&session, None, tx)
                        .map(SessionView::from),
                    Some(SessionError::HumanOwned) => {
                        // Learned from the host: it is a person's now.
                        self.session_observe(&session, reply.session.as_ref(), tx)?;
                        Err(OrchestrationError::Session(SessionError::HumanOwned))
                    }
                    Some(error) => Err(OrchestrationError::Session(error)),
                    None => Err(OrchestrationError::Observation(
                        TargetFailure::InvalidResponse,
                    )),
                };
            }
            let after = match reply.reason.as_deref() {
                // It was already gone when the stop arrived.
                Some("host_lost") => self.session_observe(&session, reply.session.as_ref(), tx)?,
                _ if session.state == SessionState::Requested => stopped("never_started", tx)?,
                _ => stopped(
                    if human {
                        "person_stopped"
                    } else {
                        "wound_down"
                    },
                    tx,
                )?,
            };
            Ok(SessionView {
                safe_point: reply.safe_point,
                ..SessionView::from(after)
            })
        })();
        match &authorized {
            Some(authorized) => self.finish(&target, authorized, &outcome, tx)?,
            None => self.session_audit(&session, actor, "session_stop", &outcome, tx)?,
        }
        outcome
    }

    /// A person takes a session over, optionally pausing it. The record changes first,
    /// so this works while the target cannot be reached; the answer then names what it
    /// could not tell the host.
    pub fn session_takeover(
        &self,
        owner_id: &str,
        session_id: &str,
        request: TakeoverSession,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<SessionView, OrchestrationError> {
        let actor_id =
            actor
                .and_then(|actor| actor.id.clone())
                .ok_or(OrchestrationError::Invalid(
                    "an identified actor is required to take a session over",
                ))?;
        if !is_human(actor) {
            return Err(OrchestrationError::Forbidden(
                "only a person takes over or pauses an orchestrator-owned session",
            ));
        }
        let session = self.session(owner_id, session_id)?;
        if session.state.is_terminal() {
            return Err(OrchestrationError::Session(SessionError::NotActive));
        }
        let control = if request.pause {
            SessionControl::Pause
        } else {
            SessionControl::Takeover
        };
        let taken = self.session_apply(
            owner_id,
            session_id,
            SessionUpdate {
                owner: Some(SessionOwner::Human),
                paused: Some(request.pause),
                control: Some(control),
                reason: Some(control.code().into()),
                actor_id: Some(actor_id),
                ..SessionUpdate::default()
            },
            false,
            tx,
        )?;
        let target = Self::target_ref(owner_id, &session.target_id)?;
        let told = self
            .person_destination(&target, tx)
            .and_then(|destination| {
                let body = serde_json::to_value(HostedSessionControl {
                    operation_id: session.operation_id.clone(),
                    pause: request.pause,
                    actor: actor.cloned(),
                })
                .map_err(|_| OrchestrationError::Corrupt)?;
                self.hosted_call_at(
                    &destination,
                    "/target-sessions",
                    "PATCH",
                    "",
                    Some(&body),
                    tx,
                )
            });
        let outcome = Ok(SessionView {
            unverified: if told.is_ok() {
                Vec::new()
            } else {
                vec!["target_session"]
            },
            ..SessionView::from(taken)
        });
        self.session_audit(
            &session,
            actor,
            if request.pause {
                "session_pause"
            } else {
                "session_takeover"
            },
            &outcome,
            tx,
        )?;
        outcome
    }
}

/// The machine a destination lives on.
fn server_of(destination: &TargetDestination) -> &str {
    match destination {
        TargetDestination::Local { server_id, .. } => server_id,
        TargetDestination::Remote { identity, .. } => &identity.server_id,
    }
}

/// The orchestration project's display name, bounded for a tab label.
pub(super) fn project_name(owner: &Checkout) -> String {
    let name: String = owner
        .alias
        .chars()
        .filter(|c| !c.is_control())
        .take(hotsheet_ticketing::orchestration::MAX_SESSION_PROJECT_CHARS)
        .collect();
    if name.trim().is_empty() {
        "Orchestration".into()
    } else {
        name
    }
}
