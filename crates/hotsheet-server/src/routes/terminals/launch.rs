//! Terminal launch, shell history, session registration, and session cleanup.

use crate::api_error::ApiError;
use crate::routes::permissions::{discovered_ai_tools, now_ms};
use crate::routes::session_messages;
use crate::routes::terminals::{
    OpenTerminalReq, TerminalInfo, broker_err, broker_info, read_terminal_settings, term_info,
    term_key, terminal_replay_bytes,
};
use crate::{AppState, multistore, now, terminal_broker, terminal_names};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use hotsheet_model::Ulid;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::{FsStore, Settings, ops};
use std::path::Path as FsPath;

/// `POST /terminals` `{command?, shell_command?, args?, cwd?, id?, connect?}` — open (or
/// reattach to) a PTY. `shell_command` runs text through the user's default shell and then keeps the
/// terminal open interactively so its output stays visible (HS2-2BKPGK); with no launch field, that
/// shell is opened interactively.
pub(crate) async fn open_terminal(
    State(state): State<AppState>,
    Json(req): Json<OpenTerminalReq>,
) -> Result<Json<TerminalInfo>, ApiError> {
    let id = req.id.clone().unwrap_or_else(|| Ulid::new().to_string());
    let launch_state = state.clone();
    let launch_id = id.clone();
    // Setup and model discovery may launch subprocesses or wait for a shared catalog
    // lock. Keep the entire synchronous preparation off Tokio workers (HS2-Y7W3Z4).
    let (launch, req, replay_bytes) = tokio::task::spawn_blocking(move || {
        let launch = terminal_launch(&launch_state, &req, &launch_id)?;
        // New terminals retain the configured replay so a reconnect restores about what the
        // client keeps (HS2-Y5DYC0); a reattach keeps the size it was spawned with.
        let replay_bytes = terminal_replay_bytes(&launch_state);
        Ok::<_, ApiError>((launch, req, replay_bytes))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    // Only a freshly spawned AI terminal registers its session-push adapter (HS2-PEJ962); a
    // reattach keeps the adapter (and port) its original launch registered.
    let push = launch.push;
    let broker_push = launch.broker_push;
    let codex_bootstrap = launch.codex_bootstrap;
    let kind = if req.connect.is_some() {
        hotsheet_terminals::TerminalKind::Ai
    } else {
        hotsheet_terminals::TerminalKind::Shell
    };
    // Record a launch directory for every terminal, even one whose shell never reports OSC 7:
    // it is what keeps a checkout's stores hosted while the terminal lives (HS2-R5KV1Q).
    let cwd = req
        .cwd
        .clone()
        .unwrap_or_else(|| state.store.root().to_string_lossy().into_owned());
    let connection_project = terminal_permission_project(&state, &req);

    // Broker mode: the PTY lives in the detached broker (survives a server restart).
    if let Some(tb) = &state.terminal_broker {
        // Newly-spawned vs reattach: a pre-check keeps the `connect` busy feed one-per-terminal
        // (no duplicate poll task on a reattach), mirroring the in-process path.
        let previously_alive = matches!(tb.info(&id).await, Ok(Some(info)) if info.alive);
        let resp = tb
            .call(hotsheet_terminals::BrokerRequest::Open {
                id: id.clone(),
                kind,
                command: launch.command,
                args: launch.args,
                cwd: Some(cwd),
                env: launch.env,
                replay_bytes: Some(replay_bytes),
                push: broker_push.map(Box::new),
            })
            .await
            .map_err(|e| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("broker: {e}"))
            })?;
        let info = match resp {
            hotsheet_terminals::BrokerResponse::Terminal { info } => info,
            other => return Err(broker_err(other)),
        };
        // A current broker reports the atomic Open decision. Older brokers do not, so
        // fall back to the pre-Open liveness check for compatibility.
        let newly_spawned = info.spawned.unwrap_or(!previously_alive);
        // Tool-in-terminal (HS2-4M67VN): register a live connection + feed its busy from the
        // broker-hosted terminal's inference (polled over the socket), once per fresh terminal.
        if newly_spawned {
            match &req.connect {
                Some(tool) => {
                    register_broker_terminal_connection(
                        &state,
                        &id,
                        tool,
                        &connection_project,
                        tb.clone(),
                    );
                    if let Some(push) = push {
                        session_messages::register(&state, &id, push);
                    } else if codex_bootstrap {
                        session_messages::register_codex_bootstrap(&state, &id);
                    }
                }
                None => watch_broker_terminal_session_exit(
                    &state,
                    &id,
                    terminal_session_worker_id(&id),
                    tb.clone(),
                ),
            }
        } else if info.kind == hotsheet_terminals::TerminalKind::Ai {
            // A fresh server may open the existing PTY before its startup resumption ran.
            // Register the connection as well as its push adapter in that case.
            let original_tool = info
                .worker
                .as_deref()
                .and_then(|worker| worker.strip_suffix(&format!("-{id}")))
                .filter(|tool| !tool.is_empty())
                .or(req.connect.as_deref());
            if let Some(tool) = original_tool {
                if state.drive_registry.lock_or_recover().get(&id).is_none() {
                    let project = terminal_project_for_cwd(&state, info.cwd.as_deref());
                    register_broker_terminal_connection(&state, &id, tool, &project, tb.clone());
                }
                if session_messages::adapter(&state, &id).is_none() {
                    restore_broker_push(&state, &id, tb).await;
                }
            }
        }
        return Ok(Json(broker_info(info)));
    }

    let spec = hotsheet_terminals::TermSpec {
        kind,
        command: launch.command,
        args: launch.args,
        cwd: Some(std::path::PathBuf::from(cwd)),
        env: launch.env,
        rows: 24,
        cols: 80,
        replay_bytes: usize::try_from(replay_bytes).unwrap_or(usize::MAX),
    };
    let (term, newly_spawned) = state
        .terminals
        .get_or_spawn_with_status(term_key(&state, &id), spec)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;

    // Tool-in-terminal (HS2-4M67VN): register a live connection + feed its busy from the
    // terminal's inference, once per fresh terminal (not on a reattach).
    if newly_spawned {
        match &req.connect {
            Some(tool) => {
                register_terminal_connection(&state, &id, tool, &connection_project, term.clone());
                if let Some(push) = push {
                    session_messages::register(&state, &id, push);
                } else if codex_bootstrap {
                    session_messages::register_codex_bootstrap(&state, &id);
                }
            }
            None => {
                watch_terminal_session_exit(&state, term.clone(), terminal_session_worker_id(&id))
            }
        }
    }
    Ok(Json(term_info(&term, &id)))
}

/// Compose the caller's explicit command, a plugin-declared interactive launch, or the user's
/// default shell. Connect-only launch performs setup first so MCP/instructions/hooks exist before
/// the tool starts, resolves the program without a shell, and injects this server's permission
/// route.
struct PreparedTerminalLaunch {
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    /// The session-push adapter for a push-capable AI launch (HS2-PEJ962).
    push: Option<std::sync::Arc<dyn hotsheet_aitools::session_push::SessionPush>>,
    broker_push: Option<hotsheet_terminals::BrokerPushState>,
    codex_bootstrap: bool,
}

fn terminal_launch(
    state: &AppState,
    req: &OpenTerminalReq,
    terminal_id: &str,
) -> Result<PreparedTerminalLaunch, ApiError> {
    if let Some(shell_command) = &req.shell_command {
        if req.command.is_some() || req.connect.is_some() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "shell_command cannot be combined with command or connect",
            ));
        }
        let command = user_default_shell();
        return Ok(PreparedTerminalLaunch {
            args: shell_command_args(shell_command, &command),
            env: terminal_session_env(state, req, terminal_id, &command)?,
            command,
            push: None,
            broker_push: None,
            codex_bootstrap: false,
        });
    }
    if let Some(command) = &req.command {
        return Ok(PreparedTerminalLaunch {
            command: command.clone(),
            args: req.args.clone(),
            env: terminal_session_env(state, req, terminal_id, command)?,
            push: None,
            broker_push: None,
            codex_bootstrap: false,
        });
    }
    let Some(tool) = req.connect.as_deref() else {
        let command = user_default_shell();
        return Ok(PreparedTerminalLaunch {
            env: terminal_session_env(state, req, terminal_id, &command)?,
            command,
            args: req.args.clone(),
            push: None,
            broker_push: None,
            codex_bootstrap: false,
        });
    };
    let root = state.store.root();
    hotsheet_aitools::launch_safety::assert_no_hs1(root)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    hotsheet_plugins::run_setup_in(root, root, Some(tool), false, None, &state.plugin_dirs)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    let plugin = hotsheet_plugins::find_in(tool, &state.plugin_dirs)
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, format!("unknown tool '{tool}'")))?;
    let launch = plugin.manifest.launch.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("plugin '{tool}' does not declare an interactive launch"),
        )
    })?;
    if req.model.is_some() || req.effort.is_some() {
        hotsheet_plugins::validate_ai_defaults(
            &discovered_ai_tools(state, false),
            &hotsheet_plugins::AiToolDefaults {
                tool: tool.to_string(),
                model: req.model.clone(),
                effort: req.effort.clone(),
                ..Default::default()
            },
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
    }
    let program = hotsheet_aitools::launch_safety::resolve_program(&launch.program)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
    let mut env = terminal_permission_route_env(state, req, terminal_id);
    env.push(("HOTSHEET_AGENT".to_string(), tool.to_string()));
    // The session's worker id: its claims are released when the terminal exits (HS2-1VAW1C).
    let worker_id = hotsheet_aitools::session_worker_id(tool, terminal_id);
    // An AI terminal session's CLI/MCP mutations act as `ai` (HS2-RD4M29).
    env.extend(
        hotsheet_aitools::ai_session_actor_env(&worker_id)
            .map(|(key, value)| (key.to_string(), value)),
    );
    env.push((hotsheet_aitools::WORKER_ID_ENV.to_string(), worker_id));
    let mut args = plugin
        .launch_args(req.model.as_deref(), req.effort.as_deref())
        .unwrap_or_default();
    // A push-capable tool is launched reachable: its declared port/password wiring is
    // appended so `POST /connections/{id}/message` can deliver into it (HS2-PEJ962).
    let directory = req
        .cwd
        .clone()
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    let push = session_messages::prepare_session_push(&plugin, &directory, req.model.as_deref())?;
    let codex_bootstrap = plugin
        .manifest
        .session_push
        .as_ref()
        .is_some_and(|spec| spec.mechanism == hotsheet_aitools::session_push::CODEX_QUEUE);
    if let Some(prepared) = &push {
        args.extend(prepared.args.iter().cloned());
        env.extend(prepared.env.iter().cloned());
    }
    Ok(PreparedTerminalLaunch {
        command: program.to_string_lossy().into_owned(),
        args,
        env,
        broker_push: push.as_ref().map(|prepared| prepared.broker_state.clone()),
        push: push.map(|prepared| prepared.adapter),
        codex_bootstrap,
    })
}

fn user_default_shell() -> String {
    #[cfg(windows)]
    let (variable, fallback) = ("COMSPEC", "cmd.exe");
    #[cfg(not(windows))]
    let (variable, fallback) = ("SHELL", "/bin/sh");

    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| fallback.to_string())
}

/// Build the shell args that run a custom command button. The terminal must stay open with the
/// output visible instead of exiting the moment the command finishes (HS2-2BKPGK): on Windows `/K`
/// runs the command and keeps the prompt (vs `/C`, which exits); on POSIX the command runs and then
/// execs an interactive login shell.
fn shell_command_args(command: &str, shell: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        let _ = shell;
        return vec![
            "/D".to_string(),
            "/S".to_string(),
            "/K".to_string(),
            command.to_string(),
        ];
    }
    #[cfg(not(windows))]
    return vec![
        "-lc".to_string(),
        format!("{command}\nexec {} -il", shell_single_quote(shell)),
    ];
}

/// Single-quote a shell word so a shell path with spaces or quotes stays one argument.
#[cfg(not(windows))]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) const INHERIT_GLOBAL_SHELL_HISTORY_SETTING: &str =
    "terminal.inherit_global_shell_history";

/// Per-terminal shell history lives under the state's injected machine home, never a
/// direct `HOTSHEET_HOME` read (HS2-FQEESP).
fn terminal_history_home(state: &AppState) -> std::path::PathBuf {
    state.machine_home().join("terminal-history")
}

fn history_key(value: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(value.as_bytes()))[..16].to_string()
}

fn shell_history_environment(
    history_home: &FsPath,
    project: &FsPath,
    terminal_id: &str,
    command: &str,
    inherit: bool,
) -> std::io::Result<Vec<(String, String)>> {
    let original = std::env::var_os("ZDOTDIR")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from));
    shell_history_environment_with_startup_dir(
        history_home,
        project,
        terminal_id,
        command,
        inherit,
        original.as_deref(),
    )
}

// Keep the real shell startup source injectable so tests never run a developer's dotfiles.
fn shell_history_environment_with_startup_dir(
    history_home: &FsPath,
    project: &FsPath,
    terminal_id: &str,
    command: &str,
    inherit: bool,
    original: Option<&FsPath>,
) -> std::io::Result<Vec<(String, String)>> {
    if inherit {
        return Ok(Vec::new());
    }
    let shell = FsPath::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command)
        .trim_start_matches('-');
    let project_key = history_key(&project.to_string_lossy());
    let terminal_key = history_key(terminal_id);
    match shell {
        "bash" => {
            let directory = history_home.join(&project_key).join(&terminal_key);
            std::fs::create_dir_all(&directory)?;
            Ok(vec![(
                "HISTFILE".into(),
                directory
                    .join(format!("{shell}_history"))
                    .to_string_lossy()
                    .into_owned(),
            )])
        }
        "zsh" => {
            let directory = history_home.join(&project_key).join(&terminal_key);
            let zdotdir = directory.join("zdotdir");
            let history = directory.join("zsh_history");
            std::fs::create_dir_all(&zdotdir)?;
            let quoted = |path: &FsPath| path.to_string_lossy().replace('\'', "'\\''");
            for name in [".zshenv", ".zprofile", ".zshrc", ".zlogin", ".zlogout"] {
                let source = original
                    .map(|root| root.join(name))
                    .filter(|path| path != &zdotdir.join(name));
                let source_line = source.map_or_else(String::new, |path| {
                    format!(
                        "[[ -r '{}' ]] && source '{}'\n",
                        quoted(&path),
                        quoted(&path)
                    )
                });
                std::fs::write(
                    zdotdir.join(name),
                    format!(
                        "{source_line}export ZDOTDIR='{}'\nexport HISTFILE='{}'\nexport SHELL_SESSIONS_DISABLE=1\n",
                        quoted(&zdotdir),
                        quoted(&history)
                    ),
                )?;
            }
            Ok(vec![
                ("HISTFILE".into(), history.to_string_lossy().into_owned()),
                ("ZDOTDIR".into(), zdotdir.to_string_lossy().into_owned()),
                ("SHELL_SESSIONS_DISABLE".into(), "1".into()),
            ])
        }
        // Fish selects a durable history file by session name. Keeping XDG_DATA_HOME intact
        // preserves the user's functions and universal variables while isolating recall.
        "fish" => Ok(vec![(
            "fish_history".into(),
            format!("hotsheet_{project_key}_{terminal_key}"),
        )]),
        _ => Ok(Vec::new()),
    }
}

/// The permission route-back every Hot Sheet terminal carries, so a hook-capable tool
/// (Claude, Codex) started by hand in a shell raises its permission prompts in the app just
/// like a Connect-launched one (HS2-HE4AVD). Without a known server URL, only the project is
/// set and the tool's native prompt stays in charge.
pub(crate) fn terminal_permission_route_env(
    state: &AppState,
    req: &OpenTerminalReq,
    terminal_id: &str,
) -> Vec<(String, String)> {
    let project = terminal_permission_project(state, req);
    let permission_identity = terminal_permission_identity(state, req.cwd.as_deref());
    let mut env = vec![
        ("HOTSHEET_SECRET".to_string(), state.secret.clone()),
        ("HOTSHEET_PROJECT".to_string(), project),
        (
            "HOTSHEET_PERMISSION_PROJECT".to_string(),
            permission_identity,
        ),
        (
            "HOTSHEET_HOME".to_string(),
            state.machine_home().display().to_string(),
        ),
        // Lets a tool's hook adapter report a halted session against this tab (HS2-HJ4D1H).
        ("HOTSHEET_TERMINAL_ID".to_string(), terminal_id.to_string()),
    ];
    if let Some(url) = state.terminal_server_url.lock_or_recover().as_ref() {
        env.push(("HOTSHEET_SERVER".to_string(), url.clone()));
    }
    env
}

pub(crate) fn terminal_permission_project(state: &AppState, req: &OpenTerminalReq) -> String {
    terminal_project_for_cwd(state, req.cwd.as_deref())
}

fn terminal_permission_identity(state: &AppState, cwd: Option<&str>) -> String {
    let cwd = cwd.unwrap_or_else(|| state.store.root().to_str().unwrap_or_default());
    let cwd = FsPath::new(cwd)
        .canonicalize()
        .unwrap_or_else(|_| cwd.into());
    let checkout = state
        .checkout_registry
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter(|checkout| cwd.starts_with(FsPath::new(&checkout.root)))
        .max_by_key(|checkout| checkout.root.len());
    checkout.map_or_else(
        || format!("unregistered:{}", cwd.display()),
        |checkout| {
            let identity = checkout.permission_identity();
            state.register_checkout_permission_rules(&identity);
            identity
        },
    )
}

fn terminal_project_for_cwd(state: &AppState, cwd: Option<&str>) -> String {
    let Some(cwd) = cwd else {
        return state.store.root().display().to_string();
    };
    let cwd = FsPath::new(cwd)
        .canonicalize()
        .unwrap_or_else(|_| cwd.into());
    let checkout = state
        .checkout_registry
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter(|checkout| cwd.starts_with(FsPath::new(&checkout.root)))
        .max_by_key(|checkout| checkout.root.len());
    if let Some(checkout) = checkout {
        let git_source = checkout
            .default_source
            .as_deref()
            .and_then(|id| checkout.source(id))
            .filter(|source| source.provider == "git")
            .or_else(|| {
                checkout
                    .sources
                    .iter()
                    .find(|source| source.provider == "git")
            });
        return git_source
            .map(|source| source.locator.clone())
            .unwrap_or(checkout.root);
    }
    cwd.display().to_string()
}

/// Shell and command terminals: [`terminal_shell_history_env`] plus the session worker id.
fn terminal_session_env(
    state: &AppState,
    req: &OpenTerminalReq,
    terminal_id: &str,
    command: &str,
) -> Result<Vec<(String, String)>, ApiError> {
    let mut env = terminal_shell_history_env(state, req, terminal_id, command)?;
    env.push((
        hotsheet_aitools::WORKER_ID_ENV.to_string(),
        terminal_session_worker_id(terminal_id),
    ));
    Ok(env)
}

/// Shell and command terminals: per-terminal shell history plus the permission route-back.
fn terminal_shell_history_env(
    state: &AppState,
    req: &OpenTerminalReq,
    terminal_id: &str,
    command: &str,
) -> Result<Vec<(String, String)>, ApiError> {
    let mut env = terminal_permission_route_env(state, req, terminal_id);
    env.extend(terminal_shell_history_only_env(
        state,
        req,
        terminal_id,
        command,
    )?);
    Ok(env)
}

fn terminal_shell_history_only_env(
    state: &AppState,
    req: &OpenTerminalReq,
    terminal_id: &str,
    command: &str,
) -> Result<Vec<(String, String)>, ApiError> {
    let inherit = read_terminal_settings(state)?.inherit_global_shell_history;
    let project = req
        .cwd
        .as_deref()
        .map(FsPath::new)
        .unwrap_or_else(|| state.store.root());
    shell_history_environment(
        &terminal_history_home(state),
        project,
        terminal_id,
        command,
        inherit,
    )
    .map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("preparing terminal history: {error}"),
        )
    })
}

/// Register a launched-in-terminal tool as a `Pty` connection on the shared registry and
/// spawn a background task that feeds the terminal's busy/idle into it until the child exits
/// (HS2-4M67VN). The connection id is the terminal id, so `GET /connections` and the terminal
/// surfaces line up.
fn register_terminal_connection(
    state: &AppState,
    id: &str,
    tool: &str,
    project: &str,
    term: std::sync::Arc<hotsheet_terminals::Terminal>,
) {
    let registry = state.drive_registry();
    let generation = {
        let mut r = registry.lock_or_recover();
        r.register_tracked(hotsheet_aitools::Connection {
            id: id.to_string(),
            project: project.to_string(),
            tool: tool.to_string(),
            role: hotsheet_aitools::Role::Main,
            transport: hotsheet_aitools::Transport::Pty,
            pid: None,
            started_at_ms: now_ms(),
        })
        .1
    };
    let conn_id = id.to_string();
    let host = state.host.clone();
    let pushes = state.session_pushes.clone();
    let worker = hotsheet_aitools::session_worker_id(tool, id);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let alive = term.is_alive();
            if !alive {
                let removed = {
                    let mut r = registry.lock_or_recover();
                    r.unregister_if_generation(&conn_id, generation)
                };
                if removed {
                    session_messages::unregister(&pushes, &conn_id);
                    release_session_claims(host, worker).await;
                }
                break;
            }
            {
                let mut r = registry.lock_or_recover();
                if !r.is_generation(&conn_id, generation) {
                    break;
                }
                match term.activity() {
                    hotsheet_terminals::Activity::Busy => r.note_activity(&conn_id, now_ms()),
                    hotsheet_terminals::Activity::Idle => r.set_idle(&conn_id),
                }
            }
        }
    });
}

/// The broker-mode counterpart of [`register_terminal_connection`] (HS2-ERT00F item 5): the
/// PTY lives in the broker, so the busy feed **polls the broker over the socket** (an `Info`
/// every 500ms, never the scrollback-carrying `Read`, HS2-7RRAA8) for the terminal's busy/idle
/// instead of reading an in-process `Terminal`. Ends
/// (and unregisters) when the terminal is gone (NotFound / not alive) or the broker is
/// unreachable.
fn register_broker_terminal_connection(
    state: &AppState,
    id: &str,
    tool: &str,
    project: &str,
    broker: terminal_broker::TerminalBroker,
) {
    let registry = state.drive_registry();
    let generation = {
        let mut r = registry.lock_or_recover();
        r.register_tracked(hotsheet_aitools::Connection {
            id: id.to_string(),
            project: project.to_string(),
            tool: tool.to_string(),
            role: hotsheet_aitools::Role::Main,
            transport: hotsheet_aitools::Transport::Pty,
            pid: None,
            started_at_ms: now_ms(),
        })
        .1
    };
    let conn_id = id.to_string();
    let host = state.host.clone();
    let pushes = state.session_pushes.clone();
    let worker = hotsheet_aitools::session_worker_id(tool, id);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            match broker.info(&conn_id).await {
                Ok(Some(info)) if info.alive => {
                    let mut r = registry.lock_or_recover();
                    if !r.is_generation(&conn_id, generation) {
                        break;
                    }
                    if info.busy {
                        r.note_activity(&conn_id, now_ms());
                    } else {
                        r.set_idle(&conn_id);
                    }
                }
                // Terminal gone (NotFound / exited) or broker unreachable → stop + unregister.
                _ => {
                    let removed = {
                        let mut r = registry.lock_or_recover();
                        r.unregister_if_generation(&conn_id, generation)
                    };
                    if removed {
                        session_messages::unregister(&pushes, &conn_id);
                        release_session_claims(host, worker).await;
                    }
                    break;
                }
            }
        }
    });
}

/// Release every claim an ended AI session's worker still holds in any hosted store: the
/// safety net for a session that stopped without releasing (HS2-1VAW1C). The store watcher
/// reindexes and announces the released tickets like any other write.
pub(crate) async fn release_session_claims(host: multistore::StoreHost, worker: String) {
    let _ =
        tokio::task::spawn_blocking(move || release_session_claims_blocking(&host, &worker)).await;
}

/// The blocking body of [`release_session_claims`], for callers already off the async runtime
/// (a chat drive's turn thread). Returns the released ticket slugs.
pub(crate) fn release_session_claims_blocking(
    host: &multistore::StoreHost,
    worker: &str,
) -> Vec<String> {
    let mut released = Vec::new();
    for (id, _) in host.locations() {
        let Some(entry) = host.get(&id) else {
            continue;
        };
        match ops::release_worker(&entry.store, now(), worker) {
            Ok(tickets) => released.extend(tickets.into_iter().map(|ticket| ticket.slug)),
            Err(error) => {
                tracing::warn!(worker, store = %id, %error, "releasing worker claims failed")
            }
        }
    }
    if !released.is_empty() {
        tracing::info!(worker, tickets = %released.join(", "), "released claims left by worker");
    }
    released
}

/// The worker id of a shell or command terminal's session. A user may start an AI tool in it
/// by hand; the tool claims with this id and the server releases it when the terminal ends
/// (HS2-RXWXQ8).
pub(crate) fn terminal_session_worker_id(terminal_id: &str) -> String {
    hotsheet_aitools::session_worker_id("terminal", terminal_id)
}

/// Tracks a shell terminal's foreground command so its session claims are released when a
/// program the user started there (such as an AI tool) exits back to the prompt, while the shell
/// stays open (HS2-WQQYT1).
#[derive(Debug, Default)]
struct ForegroundCommand {
    running: bool,
}

impl ForegroundCommand {
    /// Record the latest observation; `true` when a running command just finished. An unknown
    /// observation (`None`) changes nothing.
    fn finished(&mut self, observed: Option<bool>) -> bool {
        let Some(running) = observed else {
            return false;
        };
        let finished = self.running && !running;
        self.running = running;
        finished
    }
}

/// Release a shell or command terminal's session claims when its foreground command finishes
/// and once its in-process PTY exits.
fn watch_terminal_session_exit(
    state: &AppState,
    term: std::sync::Arc<hotsheet_terminals::Terminal>,
    worker: String,
) {
    let host = state.host.clone();
    tokio::spawn(async move {
        let mut foreground = ForegroundCommand::default();
        while term.is_alive() {
            if foreground.finished(term.foreground_command_running()) {
                release_session_claims(host.clone(), worker.clone()).await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        release_session_claims(host, worker).await;
    });
}

/// The broker-mode counterpart of [`watch_terminal_session_exit`]: poll the broker until the
/// terminal is gone, not alive, or the broker is unreachable, then release the claims.
fn watch_broker_terminal_session_exit(
    state: &AppState,
    id: &str,
    worker: String,
    broker: terminal_broker::TerminalBroker,
) {
    let host = state.host.clone();
    let id = id.to_string();
    tokio::spawn(async move {
        let mut foreground = ForegroundCommand::default();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            match broker.info(&id).await {
                Ok(Some(info)) if info.alive => {
                    if foreground.finished(info.foreground_command) {
                        release_session_claims(host.clone(), worker.clone()).await;
                    }
                }
                _ => break,
            }
        }
        release_session_claims(host, worker).await;
    });
}

/// Release the claims of chat drives the previous server run left open: drives live only in
/// server memory, so their tool sessions ended with it (HS2-VFXEF4). Each drive's store is
/// opened directly because a project's store may not be hosted yet. Returns the released slugs.
pub async fn release_orphaned_drive_sessions(state: &AppState) -> Vec<String> {
    let orphaned = state.client_drives.take_orphaned_drives();
    if orphaned.is_empty() {
        return Vec::new();
    }
    tokio::task::spawn_blocking(move || {
        let mut released = Vec::new();
        for drive in orphaned {
            match FsStore::open(&drive.store_path)
                .map_err(|error| error.to_string())
                .and_then(|store| {
                    ops::release_worker(&store, now(), &drive.worker_id)
                        .map_err(|error| error.to_string())
                }) {
                Ok(tickets) => released.extend(tickets.into_iter().map(|ticket| ticket.slug)),
                Err(error) => tracing::warn!(
                    worker = %drive.worker_id,
                    store = %drive.store_path.display(),
                    %error,
                    "releasing chat drive claims failed"
                ),
            }
        }
        if !released.is_empty() {
            tracing::warn!(
                tickets = %released.join(", "),
                "released claims left by ended chat drives"
            );
        }
        released
    })
    .await
    .unwrap_or_default()
}

/// After a restart, resume the session monitors of terminals that survived in the broker: an
/// AI terminal gets its busy feed and exit release back, a shell terminal its exit release.
/// Terminals from a broker too old to report a worker id keep relying on lease expiry.
pub async fn resume_broker_terminal_sessions(state: &AppState) {
    let Some(broker) = state.terminal_broker.clone() else {
        return;
    };
    let Ok(hotsheet_terminals::BrokerResponse::List { terminals }) =
        broker.call(hotsheet_terminals::BrokerRequest::List).await
    else {
        return;
    };
    for info in terminals.into_iter().filter(|info| info.alive) {
        let Some(worker) = info.worker else {
            continue;
        };
        let tool = worker
            .strip_suffix(&format!("-{}", info.id))
            .filter(|tool| !tool.is_empty());
        match (info.kind, tool) {
            (hotsheet_terminals::TerminalKind::Ai, Some(tool)) => {
                let project = terminal_project_for_cwd(state, info.cwd.as_deref());
                register_broker_terminal_connection(
                    state,
                    &info.id,
                    tool,
                    &project,
                    broker.clone(),
                );
                restore_broker_push(state, &info.id, &broker).await;
            }
            _ => watch_broker_terminal_session_exit(state, &info.id, worker, broker.clone()),
        }
    }
}

async fn restore_broker_push(state: &AppState, id: &str, broker: &terminal_broker::TerminalBroker) {
    if let Ok(hotsheet_terminals::BrokerResponse::PushInfo { push }) = broker
        .call(hotsheet_terminals::BrokerRequest::PushInfo { id: id.to_string() })
        .await
    {
        session_messages::restore_broker_push(state, id, push);
    }
}

/// Forget saved tab names of terminals that no longer exist (HS2-8A0FYR). Run at server startup,
/// after the broker (if any) is attached: a terminal that vanished without `DELETE
/// /terminals/{id}` — the in-process manager lost it in a restart, or the broker crashed and a
/// fresh one was spawned — must not lend its name to a later terminal reusing the id. Pruning
/// needs an authoritative live list, so an unreachable broker skips it rather than wiping every
/// name; `GET /terminals` never prunes. Returns the pruned ids.
pub async fn prune_orphaned_terminal_names(state: &AppState) -> Vec<String> {
    let live: Vec<String> = if let Some(broker) = &state.terminal_broker {
        match broker.call(hotsheet_terminals::BrokerRequest::List).await {
            Ok(hotsheet_terminals::BrokerResponse::List { terminals }) => {
                terminals.into_iter().map(|info| info.id).collect()
            }
            _ => return Vec::new(),
        }
    } else {
        state
            .terminals
            .list()
            .into_iter()
            .map(|key| key.1)
            .collect()
    };
    match terminal_names::retain_live(&Settings::new(state.store.root()), &live) {
        Ok(pruned) => pruned,
        Err(error) => {
            tracing::warn!(%error, "pruning saved terminal names failed");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod foreground_command_tests {
    use super::ForegroundCommand;

    #[test]
    fn a_finish_is_a_running_to_idle_transition_only() {
        let mut fg = ForegroundCommand::default();
        assert!(!fg.finished(Some(false)), "idle at the prompt");
        assert!(!fg.finished(None), "unknown changes nothing");
        assert!(!fg.finished(Some(true)), "a command starts");
        assert!(!fg.finished(Some(true)), "still running");
        assert!(!fg.finished(None), "a missed observation keeps it running");
        assert!(fg.finished(Some(false)), "back at the prompt: finished");
        assert!(!fg.finished(Some(false)), "reported once");
        assert!(!fg.finished(Some(true)));
        assert!(fg.finished(Some(false)), "a later command finishes too");
    }
}

#[cfg(test)]
mod terminal_history_tests {
    use super::{
        shell_command_args, shell_history_environment, shell_history_environment_with_startup_dir,
    };

    /// HS2-FQEESP: terminal history is rooted at the state's injected machine home, so two
    /// states in one process never share (or touch the real) `~/.hotsheet2` history.
    #[test]
    fn terminal_history_lives_under_the_injected_machine_home() {
        let store_dir = tempfile::tempdir().unwrap();
        let store = hotsheet_ticketing::FsStore::init(
            store_dir.path(),
            &hotsheet_ticketing::StoreMetadata::new("HS"),
        )
        .unwrap();
        let home_a = tempfile::tempdir().unwrap();
        let home_b = tempfile::tempdir().unwrap();
        let state = super::AppState::new(store, "secret".into()).unwrap();
        let state_a = state.clone().with_machine_home(home_a.path());
        let state_b = state.with_machine_home(home_b.path());
        let req: super::OpenTerminalReq = serde_json::from_str("{}").unwrap();
        for (state, home) in [(&state_a, home_a.path()), (&state_b, home_b.path())] {
            let env =
                super::terminal_shell_history_only_env(state, &req, "term-1", "/bin/bash").unwrap();
            let histfile = &env.iter().find(|(key, _)| key == "HISTFILE").unwrap().1;
            assert!(
                std::path::Path::new(histfile).starts_with(home.join("terminal-history")),
                "{histfile} must live under {}",
                home.display()
            );
        }
    }

    #[test]
    fn shell_command_uses_the_platform_shell_command_boundary() {
        let args = shell_command_args("npm run lint", "/bin/zsh");
        // The terminal keeps running after the command so its output stays visible (HS2-2BKPGK):
        // Windows uses /K (not /C), POSIX runs the command then execs an interactive login shell.
        #[cfg(windows)]
        assert_eq!(args, ["/D", "/S", "/K", "npm run lint"]);
        #[cfg(not(windows))]
        assert_eq!(args, ["-lc", "npm run lint\nexec '/bin/zsh' -il"]);
    }

    #[test]
    fn isolates_bash_and_zsh_by_project_and_terminal_with_restart_stable_paths() {
        let home = tempfile::tempdir().unwrap();
        let project_a = home.path().join("project-a");
        let project_b = home.path().join("project-b");
        let bash_a =
            shell_history_environment(home.path(), &project_a, "one", "/bin/bash", false).unwrap();
        let bash_again =
            shell_history_environment(home.path(), &project_a, "one", "/bin/bash", false).unwrap();
        let bash_other_terminal =
            shell_history_environment(home.path(), &project_a, "two", "/bin/bash", false).unwrap();
        let bash_other_project =
            shell_history_environment(home.path(), &project_b, "one", "/bin/bash", false).unwrap();
        let startup = home.path().join("original-zdotdir");
        std::fs::create_dir(&startup).unwrap();
        std::fs::write(
            startup.join(".zshrc"),
            "export HS2_HISTORY_TEST_STARTUP=loaded\n",
        )
        .unwrap();
        let zsh = shell_history_environment_with_startup_dir(
            home.path(),
            &project_a,
            "one",
            "/bin/zsh",
            false,
            Some(&startup),
        )
        .unwrap();
        assert_eq!(bash_a, bash_again);
        assert_ne!(bash_a, bash_other_terminal);
        assert_ne!(bash_a, bash_other_project);
        assert!(bash_a[0].1.ends_with("bash_history"));
        assert!(
            zsh.iter()
                .any(|(key, value)| key == "HISTFILE" && value.ends_with("zsh_history"))
        );
        assert!(
            zsh.iter()
                .any(|(key, value)| key == "SHELL_SESSIONS_DISABLE" && value == "1")
        );
        let zdotdir = zsh.iter().find(|(key, _)| key == "ZDOTDIR").unwrap();
        assert!(std::path::Path::new(&zdotdir.1).join(".zshrc").is_file());
        assert!(
            std::fs::read_to_string(std::path::Path::new(&zdotdir.1).join(".zshrc"))
                .unwrap()
                .contains(&format!("source '{}'", startup.join(".zshrc").display()))
        );
        if std::path::Path::new("/bin/zsh").is_file() {
            let histfile = &zsh.iter().find(|(key, _)| key == "HISTFILE").unwrap().1;
            // Source our wrapper explicitly; an interactive login shell loads machine/user
            // startup code and can stall this otherwise isolated history assertion.
            let output = std::process::Command::new("/bin/zsh")
                .args([
                    "-fc",
                    "source \"$ZDOTDIR/.zshrc\"; print -r -- $HISTFILE; print -r -- $HS2_HISTORY_TEST_STARTUP",
                ])
                .envs(zsh.iter().cloned())
                .output()
                .unwrap();
            let printed = String::from_utf8_lossy(&output.stdout);
            assert_eq!(
                printed.lines().collect::<Vec<_>>(),
                [histfile.as_str(), "loaded"]
            );
        }
        assert!(
            std::path::Path::new(&bash_a[0].1)
                .parent()
                .unwrap()
                .is_dir()
        )
    }
    #[test]
    fn gives_fish_a_stable_isolated_session_and_supports_global_opt_out() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("project");
        let fish = shell_history_environment(
            home.path(),
            &project,
            "terminal-1",
            "/opt/homebrew/bin/fish",
            false,
        )
        .unwrap();
        assert_eq!(fish[0].0, "fish_history");
        assert!(fish[0].1.starts_with("hotsheet_"));
        assert!(
            shell_history_environment(home.path(), &project, "terminal-1", "/bin/zsh", true)
                .unwrap()
                .is_empty()
        );
        assert!(
            shell_history_environment(home.path(), &project, "terminal-1", "python", false)
                .unwrap()
                .is_empty()
        )
    }
}
