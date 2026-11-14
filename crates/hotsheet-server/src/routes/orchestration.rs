//! HS2-4J6V46: authenticated adapters over the one shared target runtime.
use crate::{AppState, api_error::ApiError};
use axum::{
    Json,
    extract::{OriginalUri, State},
    http::{Method, StatusCode},
};
use hotsheet_cli::orchestration::{
    GateFollowState, MAX_SCHEDULER_WAIT_MS, MAX_TARGET_EVENT_WAIT_MS, OrchestrationRuntime,
    SchedulerFollowState, TargetEventPage, TargetEvents, TargetSessionEvent, TargetTicketEvent,
    WatchState, error_status, request_cleanup_now,
};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::{ProviderConfigRegistry, orchestration::OrchestrationError};
use serde_json::Value;
use std::sync::Arc;

/// The shared runtime with this server as its capacity probe (HS2-J8PRTV) and its
/// terminal host (HS2-GDJXGZ): sessions live here, so only this process can say what
/// is running or start another. The scheduler's executor (HS2-95SWQ4) is the one
/// `OrchestrationRuntime::live` attaches: the sessions this runtime owns. Call on a
/// blocking thread.
/// HS2-2DPGDY: the runtime as this server composes it, for its own event hooks.
pub(crate) fn server_runtime(
    state: &AppState,
    handle: tokio::runtime::Handle,
) -> Result<Arc<OrchestrationRuntime>, OrchestrationError> {
    runtime(state, handle)
}

pub(super) fn runtime(
    state: &AppState,
    handle: tokio::runtime::Handle,
) -> Result<Arc<OrchestrationRuntime>, OrchestrationError> {
    let probe = Arc::new(crate::host_health::capacity::ServerCapacity {
        state: state.clone(),
        runtime: handle.clone(),
    });
    let sessions = Arc::new(crate::routes::orchestration_sessions::ServerSessions {
        state: state.clone(),
        runtime: handle,
    });
    // HS2-XHQ1EH: waiting permission requests are shown in this server's bridge.
    let prompt = Arc::new(crate::routes::orchestration_permissions::BridgePrompt {
        state: state.clone(),
    });
    if let Some(runtime) = &state.orchestration_runtime {
        // HS2-95SWQ4: an injected runtime keeps the executor it was given.
        return Ok(Arc::new(
            OrchestrationRuntime::clone(runtime)
                .with_capacity_probe(probe)
                .with_session_host(sessions)
                .with_permission_prompt(prompt),
        ));
    }
    OrchestrationRuntime::live(
        state.machine_home(),
        state.checkout_registry.clone(),
        ProviderConfigRegistry::new(state.store.root().join("providers.json")),
    )
    .map(|runtime| {
        runtime
            .with_target_events(target_events(state))
            .with_capacity_probe(probe)
            .with_session_host(sessions)
            .with_permission_prompt(prompt)
    })
    .map(Arc::new)
}

/// HS2-7QHWHJ: this server's ticket changes as a cursor feed for orchestration
/// projects that follow one of its checkouts.
pub(crate) fn target_events(state: &AppState) -> Arc<dyn TargetEvents> {
    Arc::new(ServerEvents(state.clone()))
}

struct ServerEvents(AppState);

impl TargetEvents for ServerEvents {
    fn poll(
        &self,
        checkout: &hotsheet_ticketing::checkouts::Checkout,
        since: Option<u64>,
        wait_ms: u64,
    ) -> Result<TargetEventPage, OrchestrationError> {
        let state = &self.0;
        // Ticket changes are published under the git connection id of their store.
        let sources: std::collections::HashSet<&str> = checkout
            .sources
            .iter()
            .filter(|source| source.provider == "git")
            .map(|source| source.connection_id.as_str())
            .collect();
        // HS2-YC2P7K: a claim or a release is a ticket change a run follows: it is
        // how a dispatcher-mode run learns which worker took its ticket.
        let ticket_change = |event: &crate::ChangeEvent| {
            matches!(
                event.kind.as_str(),
                "created" | "updated" | "deleted" | "changed" | "claimed" | "released" | "moved"
            ) && sources.contains(event.store.as_str())
        };
        // HS2-257Q2Z: a session an orchestrator started in this checkout ended or
        // finished a turn.
        let session_change = |event: &crate::ChangeEvent| {
            event.kind == crate::routes::orchestration_sessions::SESSION_CHANGE_KIND
                && event.store == checkout.id
        };
        let mut page = TargetEventPage {
            cursor: 0,
            events: Vec::new(),
            sessions: Vec::new(),
            overflow: false,
            // HS2-FJ6MDW: filled by the shared runtime after the wait.
            speculation: None,
        };
        // Whether the event was one this checkout's followers want.
        let take = |page: &mut TargetEventPage, event: &crate::ChangeEvent| {
            let Some(cursor) = event.cursor else {
                return false;
            };
            if ticket_change(event) {
                page.events.push(TargetTicketEvent {
                    cursor,
                    connection_id: event.store.clone(),
                    native_id: event.id.clone(),
                });
            } else if session_change(event) {
                page.sessions.push(TargetSessionEvent {
                    cursor,
                    session_id: event.id.clone(),
                });
            } else {
                return false;
            }
            true
        };
        let Some(since) = since else {
            page.cursor = state.event_cursor();
            return Ok(page);
        };
        // Subscribe before reading the log, so a change in between is not missed.
        let mut receiver = state.events.subscribe();
        let (backlog, overflow, mut cursor) = state.event_log.with_lock(|log| {
            let (events, overflow) = log.since(since);
            (events, overflow, log.seq)
        });
        page.overflow = overflow;
        let mut found = false;
        for event in &backlog {
            found |= take(&mut page, event);
        }
        if !found && !page.overflow && wait_ms > 0 {
            // Wait for the next change of this checkout, woken by the stream itself.
            let Ok(handle) = tokio::runtime::Handle::try_current() else {
                page.cursor = cursor;
                return Ok(page);
            };
            let waited = handle.block_on(async {
                tokio::time::timeout(std::time::Duration::from_millis(wait_ms), async {
                    loop {
                        match receiver.recv().await {
                            Ok(event) if event.cursor.is_some_and(|at| at <= cursor) => {}
                            Ok(event) => {
                                if let Some(at) = event.cursor {
                                    cursor = cursor.max(at);
                                }
                                if ticket_change(&event) || session_change(&event) {
                                    return Some(Ok(event));
                                }
                            }
                            // The receiver fell behind: the caller must catch up fully.
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                return Some(Err(()));
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                        }
                    }
                })
                .await
            });
            match waited {
                Ok(Some(Ok(event))) => {
                    take(&mut page, &event);
                }
                Ok(Some(Err(()))) => {
                    page.overflow = true;
                    cursor = state.event_cursor();
                }
                Ok(None) | Err(_) => {}
            }
        }
        let last = page
            .events
            .iter()
            .map(|event| event.cursor)
            .chain(page.sessions.iter().map(|event| event.cursor))
            .max();
        page.cursor = last.map_or(cursor, |last| last.max(cursor));
        Ok(page)
    }
}

/// Followers of targets with unfinished runs, one per `(server, machine home, owner,
/// target)` (HS2-KM9CTG).
static WATCHERS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::OnceLock::new();

/// Start a follower for every target of this owner that has an unfinished run, or an
/// unfinished session (HS2-257Q2Z), and no follower yet. A follower holds one bounded
/// request open on its target, syncs the runs whose tickets changed, wakes the owner's
/// scheduler for a session change no run event reports, backs off after a failure, and
/// ends when the target has neither. With none, nothing is started and no target is
/// contacted.
fn ensure_watchers(state: &AppState, owner: &str) {
    let Ok(shared) = runtime(state, tokio::runtime::Handle::current()) else {
        return;
    };
    let mut targets: std::collections::BTreeSet<String> = shared
        .run_list(owner, false)
        .unwrap_or_default()
        .into_iter()
        .filter(|run| !run.state.is_terminal())
        .map(|run| run.target.target_id)
        .collect();
    targets.extend(
        shared
            .session_list(owner, true)
            .unwrap_or_default()
            .into_iter()
            .map(|session| session.target_id),
    );
    // HS2-TMA4YA: a target whose tickets an unfinished plan step names, with or without
    // a run on them.
    targets.extend(shared.planned_targets(owner));
    for target in targets {
        let key = format!("{}|{target}", loop_key(state, owner));
        let registry = WATCHERS.get_or_init(Default::default);
        if !registry.lock_or_recover().insert(key.clone()) {
            continue;
        }
        let (state, owner) = (state.clone(), owner.to_owned());
        tokio::spawn(async move {
            let mut watch = WatchState::default();
            let mut backoff = std::time::Duration::from_secs(1);
            // HS2-KM9CTG: a stopping server follows nothing more.
            while !state.is_stopping() {
                let (task_state, task_owner, task_target) =
                    (state.clone(), owner.clone(), target.clone());
                let step = tokio::task::spawn_blocking(move || {
                    let step =
                        runtime(&task_state, tokio::runtime::Handle::current()).map(|runtime| {
                            runtime.watch_step_now(
                                &task_owner,
                                &task_target,
                                &mut watch,
                                MAX_TARGET_EVENT_WAIT_MS,
                            )
                        });
                    (step, watch)
                })
                .await;
                let Ok((Ok(step), returned)) = step else {
                    break;
                };
                watch = returned;
                if !step.active {
                    break;
                }
                if step.failed {
                    // Bounded backoff; the watch keeps its cursor, so nothing is missed.
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(std::time::Duration::from_secs(60));
                } else {
                    backoff = std::time::Duration::from_secs(1);
                }
            }
            WATCHERS
                .get_or_init(Default::default)
                .lock_or_recover()
                .remove(&key);
        });
    }
}

/// HS2-KM9CTG: the registry key of one server's background task for an owner. It
/// names the server instance as well as the machine home, so a server started after
/// another over the same home in one process runs its own tasks, while each of them
/// ends at its next step once its own server is stopping.
pub(super) fn loop_key(state: &AppState, owner: &str) -> String {
    format!(
        "{}|{:p}|{owner}",
        state.machine_home().display(),
        Arc::as_ptr(&state.lifecycle)
    )
}

/// Gate followers, one per `(server, machine home, owner)` (HS2-KM9CTG).
static GATE_FOLLOWERS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::OnceLock::new();

/// HS2-5JBYPV: follow the owner's gates that still wait on a provider. The follower
/// holds one bounded wait open on the provider adapter's own records (for GitHub
/// Actions, the watch Job store), records each change as a run event, and ends when no
/// gate waits any more. With no such gate nothing is started.
fn ensure_gate_follower(state: &AppState, owner: &str) {
    let key = loop_key(state, owner);
    if !GATE_FOLLOWERS
        .get_or_init(Default::default)
        .lock_or_recover()
        .insert(key.clone())
    {
        return;
    }
    let (state, owner) = (state.clone(), owner.to_owned());
    tokio::spawn(async move {
        let mut follow = GateFollowState::default();
        let mut backoff = std::time::Duration::from_secs(1);
        // HS2-KM9CTG: a stopping server follows nothing more.
        while !state.is_stopping() {
            let (task_state, task_owner) = (state.clone(), owner.clone());
            let started = std::time::Instant::now();
            let step = tokio::task::spawn_blocking(move || {
                let step = runtime(&task_state, tokio::runtime::Handle::current()).map(|runtime| {
                    runtime.gate_follow_step_now(&task_owner, &mut follow, MAX_TARGET_EVENT_WAIT_MS)
                });
                (step, follow)
            })
            .await;
            let Ok((Ok(step), returned)) = step else {
                break;
            };
            follow = returned;
            if !step.active {
                break;
            }
            // A step that neither waited nor learned anything (a denied gate, an
            // adapter that cannot wait) backs off instead of spinning.
            if step.changed.is_empty() && started.elapsed() < std::time::Duration::from_millis(250)
            {
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(std::time::Duration::from_secs(60));
            } else {
                backoff = std::time::Duration::from_secs(1);
            }
        }
        GATE_FOLLOWERS
            .get_or_init(Default::default)
            .lock_or_recover()
            .remove(&key);
    });
}

/// Scheduling loops, one per `(server, machine home, owner)` (HS2-KM9CTG).
static SCHEDULERS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::OnceLock::new();

/// HS2-ECWG50: run the owner's scheduling loop while its policy enables automatic
/// placement. Each step waits on the owner's run event stream and the scheduler's
/// wake marker, runs one pass when something changed, and ends when the policy no
/// longer enables it. A policy without automatic placement ends the task at its first
/// step, having started nothing and contacted no target. Call inside the runtime.
fn ensure_scheduler(state: &AppState, owner: &str) {
    let key = loop_key(state, owner);
    if !SCHEDULERS
        .get_or_init(Default::default)
        .lock_or_recover()
        .insert(key.clone())
    {
        return;
    }
    let (state, owner) = (state.clone(), owner.to_owned());
    tokio::spawn(async move {
        let mut follow = SchedulerFollowState::default();
        let mut backoff = std::time::Duration::from_secs(1);
        // HS2-KM9CTG: a stopping server places nothing more.
        while !state.is_stopping() {
            let (task_state, task_owner) = (state.clone(), owner.clone());
            let started = std::time::Instant::now();
            let step = tokio::task::spawn_blocking(move || {
                let step = runtime(&task_state, tokio::runtime::Handle::current()).map(|runtime| {
                    runtime.scheduler_follow_step_now(
                        &task_owner,
                        &mut follow,
                        MAX_SCHEDULER_WAIT_MS,
                    )
                });
                (step, follow)
            })
            .await;
            let Ok((Ok(step), returned)) = step else {
                break;
            };
            follow = returned;
            // HS2-YC2P7K: a pass may have dispatched runs. Their target is followed
            // like any other unfinished run's, so a claim or a completion there is
            // learned from the target's change feed.
            if step.trigger.is_some() {
                // HS2-W27N1D: a pass may have started or wound down sessions; clients
                // showing this owner's orchestration read it again.
                if !step.failed {
                    note_orchestration_change(&state, &owner);
                }
                let (state, owner) = (state.clone(), owner.clone());
                let _ =
                    tokio::task::spawn_blocking(move || ensure_watchers_blocking(&state, &owner))
                        .await;
            }
            if !step.active {
                break;
            }
            // A failed pass, or a step that neither waited nor planned, backs off
            // instead of spinning. The follower keeps its cursor, so nothing is missed.
            let idle =
                step.trigger.is_none() && started.elapsed() < std::time::Duration::from_millis(250);
            if step.failed || idle {
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(std::time::Duration::from_secs(60));
            } else {
                backoff = std::time::Duration::from_secs(1);
            }
        }
        SCHEDULERS
            .get_or_init(Default::default)
            .lock_or_recover()
            .remove(&key);
    });
}

/// The owning checkout of a request that may have turned automatic placement on, or
/// that asks the scheduler something: its loop is started when the policy enables it.
fn scheduled_owner(method: &Method, path: &str) -> Option<String> {
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method.as_str(), parts.as_slice()) {
        // HS2-J3ZF27: a switch back to Orchestration resumes a kept automatic policy.
        (
            "PUT",
            [
                "checkouts",
                owner,
                "orchestration",
                "policy" | "project-type",
            ],
        )
        | ("POST", ["checkouts", owner, "orchestration", "scheduler", _]) => {
            Some((*owner).to_owned())
        }
        _ => None,
    }
}

impl AppState {
    /// HS2-WBREDM: at server start, follow every target that still has an unfinished
    /// run. A follower's first step reconciles each of those runs with its target, so
    /// nothing is resumed on a record that went stale while this server was down. With
    /// no unfinished run nothing is started and no target is contacted. Call once,
    /// after the runtime is up and this process is the serving instance.
    pub fn spawn_run_recovery(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                let Ok(handle) = tokio::runtime::Handle::try_current() else {
                    return;
                };
                let Ok(shared) = runtime(&state, handle) else {
                    return;
                };
                for owner in shared.run_owners().unwrap_or_default() {
                    ensure_watchers_blocking(&state, &owner);
                }
                // HS2-VVHVF0: and every owner with a gate that still waits on a
                // provider, whether or not its run is finished. The follower's first
                // step makes sure the Job monitor observes the watch.
                for owner in shared.gate_owners() {
                    ensure_watchers_blocking(&state, &owner);
                }
                // HS2-TMA4YA: and every owner whose plan names target tickets.
                for owner in shared.plan_owners() {
                    ensure_watchers_blocking(&state, &owner);
                }
                // HS2-ECWG50: and the scheduling loop of every owner whose policy
                // enables automatic placement. With none, nothing is started.
                for owner in shared.scheduler_owners() {
                    ensure_scheduler_blocking(&state, &owner);
                }
                // HS2-XHQ1EH: permission requests still waiting show again.
                shared.show_waiting_permissions(crate::routes::permissions::now_ms());
            })
            .await;
        });
    }
}

/// The owning checkout of a path that should have its targets followed: a dispatch,
/// a gate link or deployment request, or someone waiting on the run event stream.
fn watched_owner(method: &Method, path: &str) -> Option<String> {
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method.as_str(), parts.as_slice()) {
        ("POST", ["checkouts", owner, "orchestration", "runs"])
        | (
            "POST",
            [
                "checkouts",
                owner,
                "orchestration",
                "runs",
                _,
                "gates" | "deployments",
            ],
        )
        | ("GET", ["checkouts", owner, "orchestration", "run-events"])
        // HS2-J89JC7: a timeline reader waits on the same run events.
        | ("GET", ["checkouts", owner, "orchestration", "timeline"]) => Some((*owner).to_owned()),
        // HS2-TMA4YA: a defined or refreshed plan step's tickets are followed from then on.
        ("POST", ["checkouts", owner, "orchestration", "readiness", "define" | "refresh"]) => {
            Some((*owner).to_owned())
        }
        // HS2-WBREDM: a run that was resumed, handed off or corrected back to life, or
        // an explicit reconciliation, is followed from then on.
        ("POST", ["checkouts", owner, "orchestration", "run-reconcile"])
        | (
            "POST",
            [
                "checkouts",
                owner,
                "orchestration",
                "runs",
                _,
                "reconcile" | "resume" | "handoff" | "correct",
            ],
        ) => Some((*owner).to_owned()),
        _ => None,
    }
}
/// HS2-W27N1D: the change-event kind that tells clients an orchestration project's
/// settings changed: its type, its projects and bindings, its policy or its sessions.
/// `store` and `id` are the owning checkout. Identifiers only; a client reads again.
pub(crate) const ORCHESTRATION_CHANGE_KIND: &str = "orchestration_changed";

/// The owner of a request that changes what Settings → Orchestration shows. Reads,
/// including the reads sent as POST (capacity, admission, validate, dry run), are not.
fn changed_owner(method: &Method, path: &str) -> Option<String> {
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let owner = match (method.as_str(), parts.as_slice()) {
        (
            "PUT",
            [
                "checkouts",
                owner,
                "orchestration",
                "project-type" | "policy" | "targets",
            ],
        )
        | (
            "POST",
            [
                "checkouts",
                owner,
                "orchestration",
                "projects" | "targets" | "sessions",
            ],
        )
        | ("DELETE", ["checkouts", owner, "orchestration", "projects", _])
        | ("PATCH" | "DELETE", ["checkouts", owner, "orchestration", "targets", _])
        | ("PUT" | "DELETE", ["checkouts", owner, "orchestration", "targets", _, "binding"])
        | ("POST", ["checkouts", owner, "orchestration", "session-sync"])
        | (
            "POST",
            [
                "checkouts",
                owner,
                "orchestration",
                "sessions",
                _,
                "stop" | "takeover" | "sync",
            ],
        ) => owner,
        _ => return None,
    };
    Some((*owner).to_owned())
}

/// HS2-W27N1D: tell every client following the owner that its orchestration changed.
pub(crate) fn note_orchestration_change(state: &AppState, owner: &str) {
    state.emit(crate::ChangeEvent {
        live: None,
        cursor: None,
        store: owner.to_owned(),
        kind: ORCHESTRATION_CHANGE_KIND.into(),
        id: owner.to_owned(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    });
}

/// The API error for a runtime refusal. HS2-XHQ1EH: a refusal a person can answer
/// carries the record to answer, `request_id` or `note_id`.
pub(crate) fn orchestration_error(error: OrchestrationError) -> ApiError {
    let api = ApiError::new(
        StatusCode::from_u16(error_status(&error)).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        error.to_string(),
    )
    .with_code(error.code());
    match error.answer_reference() {
        Some((field, id)) => api.with_field(field, id),
        None => api,
    }
}
async fn execute(
    state: AppState,
    method: Method,
    path: String,
    body: Value,
) -> Result<Json<Value>, ApiError> {
    // HS2-7QHWHJ: a subscriber gets followers before it starts waiting.
    let watched = watched_owner(&method, &path);
    if let (Some(owner), "GET") = (&watched, method.as_str()) {
        let (state, owner) = (state.clone(), owner.clone());
        let _ = tokio::task::spawn_blocking(move || ensure_watchers_blocking(&state, &owner)).await;
    }
    let follow = watched.map(|owner| (state.clone(), owner));
    let scheduled = scheduled_owner(&method, &path).map(|owner| (state.clone(), owner));
    let changed = changed_owner(&method, &path).map(|owner| (state.clone(), owner));
    // HS2-SHHGC6: a reader of the owner's settings gets its files watched, so a change
    // a CLI process writes without this server is announced too.
    let settings = super::orchestration_settings_watch::settings_owner(&method, &path)
        .map(|owner| (state.clone(), owner));
    // HS2-2EKDSM: a change this server makes is in flight until it is recorded.
    if let Some((state, owner)) = &changed {
        super::orchestration_settings_watch::begin_change(state, owner);
    }
    let result = execute_blocking(state, method, path, body).await;
    if let (Err(_), Some((state, owner))) = (&result, &changed) {
        super::orchestration_settings_watch::change_failed(state, owner);
    }
    if let (Ok(_), Some((state, owner))) = (&result, settings) {
        let _ = tokio::task::spawn_blocking(move || {
            super::orchestration_settings_watch::ensure_settings_watcher(&state, &owner);
        })
        .await;
    }
    if let (Ok(_), Some((state, owner))) = (&result, changed) {
        let (seen_state, seen_owner) = (state.clone(), owner.clone());
        let _ = tokio::task::spawn_blocking(move || {
            super::orchestration_settings_watch::seen_after_change(&seen_state, &seen_owner);
        })
        .await;
        note_orchestration_change(&state, &owner);
    }
    // HS2-ECWG50: a policy that now enables automatic placement gets its loop.
    if let (Ok(_), Some((state, owner))) = (&result, scheduled) {
        let _ =
            tokio::task::spawn_blocking(move || ensure_scheduler_blocking(&state, &owner)).await;
    }
    // A new run gets a follower once it exists.
    if let (Ok(_), Some((state, owner))) = (&result, follow) {
        let _ = tokio::task::spawn_blocking(move || ensure_watchers_blocking(&state, &owner)).await;
    }
    result
}

impl AppState {
    /// HS2-DX1PQB: tell clients when a person takes over, pauses or stops an
    /// orchestrator session hosted on this machine, whichever process recorded it (this
    /// server's hosted route or a headless CLI). One thread waits on the runtime's
    /// hosted-session signal with the platform file watcher and publishes
    /// `terminal_orchestrator`; every client then refetches its terminals. Idle, it
    /// publishes nothing and makes no request. It ends when the server stops. Call
    /// inside the runtime, once per serving process.
    pub fn spawn_hosted_session_follower(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let watcher = self.clone();
        // HS2-TWSXK7: the baseline is read here, before the thread starts its runtime and
        // platform watcher, so a takeover recorded meanwhile is announced, not taken as
        // already seen.
        let directory = self
            .machine_home()
            .join(hotsheet_cli::orchestration::HOSTED_SESSION_SIGNAL_DIRECTORY);
        let seen = hotsheet_cli::orchestration::signal_token(&directory);
        let _ = std::thread::Builder::new()
            .name("orchestration-hosted-session-signal".into())
            .spawn(move || {
                let Ok(mut waiter) =
                    hotsheet_cli::orchestration::SignalWaiter::since(&directory, seen)
                else {
                    return;
                };
                while !watcher.is_stopping() {
                    if waiter.wait(std::time::Duration::from_secs(1)) {
                        announce_terminal_orchestrator(&watcher, "");
                    }
                }
            });
    }
}

/// HS2-6MNMVB: a person took over, paused or stopped an orchestrator session hosted
/// here; every client refetches its terminals so the drawer shows the new owner.
fn announce_terminal_orchestrator(state: &AppState, terminal_id: &str) {
    state.emit(crate::ChangeEvent {
        live: None,
        cursor: None,
        store: String::new(),
        kind: "terminal_orchestrator".into(),
        id: terminal_id.to_owned(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    });
}

/// `ensure_watchers` reads run records and spawns tasks; it runs off the async threads
/// and re-enters the runtime only to spawn.
fn ensure_watchers_blocking(state: &AppState, owner: &str) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        let _guard = handle.enter();
        ensure_watchers(state, owner);
        ensure_gate_follower(state, owner);
    }
}

fn ensure_scheduler_blocking(state: &AppState, owner: &str) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        let _guard = handle.enter();
        ensure_scheduler(state, owner);
    }
}

async fn execute_blocking(
    state: AppState,
    method: Method,
    path: String,
    body: Value,
) -> Result<Json<Value>, ApiError> {
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        // Must precede runtime/identity/provider construction, including for an orphan
        // owner that the normal checkout router can no longer resolve.
        if let Some(result) =
            request_cleanup_now(state.machine_home(), method.as_str(), &path, &body)
        {
            return result;
        }
        runtime(&state, handle)?.request_now(method.as_str(), &path, &body)
    })
    .await
    .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "target task failed"))?
    .map(Json)
    .map_err(orchestration_error)
}
pub(crate) async fn read(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
) -> Result<Json<Value>, ApiError> {
    // HS2-SBQ3T8: the audit cursor travels in the query; the runtime parses it.
    let path = uri
        .path_and_query()
        .map_or_else(|| uri.path().to_owned(), |path| path.as_str().to_owned());
    execute(state, Method::GET, path, Value::Null).await
}
pub(crate) async fn write(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    execute(state, method, uri.path().into(), body).await
}
