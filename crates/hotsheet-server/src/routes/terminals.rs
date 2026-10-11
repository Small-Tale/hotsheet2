//! Terminal routes and the live attach socket (HS2-A6R5QV).

use crate::*;

// ---- terminals (HS2-A6R5QV) ------------------------------------------------------

/// Body for `POST /terminals`: what to run.
#[derive(Deserialize)]
pub(crate) struct OpenTerminalReq {
    /// The program to spawn (e.g. `bash`, `codex`).
    pub(crate) command: Option<String>,
    /// Command text for the user's default shell to execute.
    pub(crate) shell_command: Option<String>,
    #[serde(default)]
    pub(crate) args: Vec<String>,
    /// Working directory (defaults to the served store root).
    pub(crate) cwd: Option<String>,
    /// Client-chosen terminal id; a ULID is minted when omitted.
    pub(crate) id: Option<String>,
    /// When set, register this terminal as a live AI-tool **connection** for that plugin id
    /// (e.g. `claude`), so `GET /connections` shows it and its busy is fed from the terminal's
    /// OSC-133 / spinner inference (HS2-4M67VN). Omit for a plain shell terminal.
    #[serde(default)]
    pub(crate) connect: Option<String>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) effort: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct TerminalSettings {
    #[serde(default)]
    pub(crate) inherit_global_shell_history: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct TrashSettings {
    pub(crate) trash_cleanup_days: u32,
}

pub(crate) fn read_terminal_settings(state: &AppState) -> Result<TerminalSettings, ApiError> {
    read_terminal_settings_from(&Settings::new(state.store.root()))
}

pub(crate) fn read_terminal_settings_from(
    settings: &Settings,
) -> Result<TerminalSettings, ApiError> {
    let inherit_global_shell_history = settings
        .get(
            INHERIT_GLOBAL_SHELL_HISTORY_SETTING,
            hotsheet_ticketing::Scope::Local,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    Ok(TerminalSettings {
        inherit_global_shell_history,
    })
}

pub(crate) async fn get_terminal_settings(
    State(state): State<AppState>,
) -> Result<Json<TerminalSettings>, ApiError> {
    Ok(Json(read_terminal_settings(&state)?))
}

pub(crate) async fn put_terminal_settings(
    State(state): State<AppState>,
    Json(value): Json<TerminalSettings>,
) -> Result<Json<TerminalSettings>, ApiError> {
    Settings::new(state.store.root())
        .set(
            INHERIT_GLOBAL_SHELL_HISTORY_SETTING,
            serde_json::Value::Bool(value.inherit_global_shell_history),
            hotsheet_ticketing::Scope::Local,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(value))
}

pub(crate) async fn get_checkout_terminal_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<TerminalSettings>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    Ok(Json(read_terminal_settings_from(&settings)?))
}

pub(crate) async fn put_checkout_terminal_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(value): Json<TerminalSettings>,
) -> Result<Json<TerminalSettings>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    settings
        .set(
            INHERIT_GLOBAL_SHELL_HISTORY_SETTING,
            serde_json::Value::Bool(value.inherit_global_shell_history),
            hotsheet_ticketing::Scope::Local,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(value))
}

pub(crate) async fn get_checkout_trash_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<TrashSettings>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let trash_cleanup_days = settings
        .trash_cleanup_days()
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(TrashSettings { trash_cleanup_days }))
}

pub(crate) async fn put_checkout_trash_settings(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(value): Json<TrashSettings>,
) -> Result<Json<TrashSettings>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    settings
        .set(
            hotsheet_ticketing::TRASH_CLEANUP_DAYS_SETTING,
            serde_json::json!(value.trash_cleanup_days),
            hotsheet_ticketing::Scope::Shared,
        )
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(value))
}

/// One terminal as reported by `GET /terminals`.
#[derive(Serialize)]
pub(crate) struct TerminalInfo {
    pub(crate) id: String,
    /// Immutable creation kind; never inferred from terminal output or command names.
    pub(crate) kind: hotsheet_terminals::TerminalKind,
    /// The PTY is still running.
    pub(crate) alive: bool,
    /// Inferred busy (a tool is actively working) vs idle.
    pub(crate) busy: bool,
    /// The shell's reported working directory (OSC 7), if any (HS2-RCKEJ9).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cwd: Option<String>,
    /// A currently-open hyperlink URI (OSC 8), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) link: Option<String>,
    /// Tool progress percent 0-100 (OSC 9;4), if reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) progress: Option<u8>,
    /// The AI tool an `ai` terminal launched (for example `claude`), from its immutable
    /// `<tool>-<id>` worker id; clients name AI tabs after it (HS2-HZK0NK).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool: Option<String>,
    /// The user's saved tab name (HS2-89FPV1), absent when the terminal was never renamed;
    /// clients then derive a default name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    /// The terminal's AI session halted on an API error and is waiting for the user
    /// (HS2-HJ4D1H); absent while it is running normally.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) halt: Option<TerminalHalt>,
    /// An interactive question from the AI session awaiting an answer in this terminal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) question: Option<TerminalQuestion>,
    /// An AI session in this terminal started with Hot Sheet's hooks active, so its permission
    /// prompts come to Hot Sheet (HS2-EV1XK3); absent when no session has reported in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ai_connection: Option<TerminalAiConnection>,
    /// Most recent trusted hook report, even if that session later ended.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_hook_report: Option<TerminalAiConnection>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalHookSource {
    #[default]
    SessionStart,
    PermissionRequest,
}

/// An AI session whose `SessionStart` or interactive permission hook reported in from a terminal
/// (HS2-EV1XK3, HS2-XYSXVT). A tool runs a project hook only once it is installed and trusted,
/// so this proves the hook is live.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalAiConnection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) agent: Option<String>,
    /// RFC 3339 time the session reported in.
    #[serde(default)]
    pub(crate) at: String,
    #[serde(default)]
    pub(crate) source: TerminalHookSource,
    /// Internal lifecycle identity; the terminal API need not expose Codex/Claude session ids.
    #[serde(skip)]
    pub(crate) session_id: Option<String>,
}

/// Why a terminal's AI session stopped: the tool's error category and message (for example
/// Claude Code's `StopFailure` `overloaded` / "Selected model is at capacity").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalHalt {
    pub(crate) error_type: String,
    pub(crate) message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) agent: Option<String>,
    /// RFC 3339 time the halt was reported.
    #[serde(default)]
    pub(crate) at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalQuestion {
    pub(crate) question: String,
    pub(crate) tool_use_id: String,
    #[serde(default)]
    pub(crate) questions: serde_json::Value,
    #[serde(skip)]
    pub(crate) session_id: Option<String>,
    #[serde(skip)]
    pub(crate) answer: Option<serde_json::Value>,
    pub(crate) at: String,
}

/// The AI tool of an `ai` terminal, recovered from its `<tool>-<id>` worker id. Shell terminals,
/// workers that do not follow that shape, and an explicit-command terminal that also connects a
/// tool (whose session worker is in the reserved `terminal` namespace) have none.
pub(crate) fn ai_terminal_tool(
    kind: hotsheet_terminals::TerminalKind,
    worker: Option<&str>,
    id: &str,
) -> Option<String> {
    if kind != hotsheet_terminals::TerminalKind::Ai {
        return None;
    }
    worker?
        .strip_suffix(&format!("-{id}"))
        .filter(|tool| !tool.is_empty() && *tool != "terminal")
        .map(str::to_string)
}

/// The terminal-manager key for a terminal id — the served store root is the project.
pub(crate) fn term_key(state: &AppState, id: &str) -> hotsheet_terminals::TermKey {
    (state.store.root().display().to_string(), id.to_string())
}

pub(crate) fn term_info(term: &hotsheet_terminals::Terminal, id: &str) -> TerminalInfo {
    let osc = term.term_state();
    TerminalInfo {
        id: id.to_string(),
        kind: term.kind(),
        alive: term.is_alive(),
        busy: term.activity() == hotsheet_terminals::Activity::Busy,
        cwd: osc.cwd,
        link: osc.link,
        progress: osc.progress,
        tool: ai_terminal_tool(term.kind(), term.worker_id(), id),
        name: None,
        halt: None,
        question: None,
        ai_connection: None,
        last_hook_report: None,
    }
}

/// Map a broker terminal-info onto the HTTP `TerminalInfo`.
pub(crate) fn broker_info(bi: hotsheet_terminals::BrokerTermInfo) -> TerminalInfo {
    let tool = ai_terminal_tool(bi.kind, bi.worker.as_deref(), &bi.id);
    TerminalInfo {
        id: bi.id,
        kind: bi.kind,
        alive: bi.alive,
        busy: bi.busy,
        cwd: bi.cwd,
        link: bi.link,
        progress: bi.progress,
        tool,
        name: None,
        halt: None,
        question: None,
        ai_connection: None,
        last_hook_report: None,
    }
}

/// Map a non-success broker response to an `ApiError`.
pub(crate) fn broker_err(resp: hotsheet_terminals::BrokerResponse) -> ApiError {
    use hotsheet_terminals::BrokerResponse as R;
    match resp {
        R::Err { message } => ApiError::new(StatusCode::BAD_REQUEST, message),
        R::NotFound => ApiError::new(StatusCode::NOT_FOUND, "no such terminal"),
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("unexpected broker response: {other:?}"),
        ),
    }
}

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
    let (launch, req) = tokio::task::spawn_blocking(move || {
        terminal_launch(&launch_state, &req, &launch_id).map(|launch| (launch, req))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
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

    // Broker mode: the PTY lives in the detached broker (survives a server restart).
    if let Some(tb) = &state.terminal_broker {
        // Newly-spawned vs reattach: a pre-check keeps the `connect` busy feed one-per-terminal
        // (no duplicate poll task on a reattach), mirroring the in-process path.
        let newly_spawned = !matches!(
            tb.call(hotsheet_terminals::BrokerRequest::Read { id: id.clone() })
                .await,
            Ok(hotsheet_terminals::BrokerResponse::Read { .. })
        );
        let resp = tb
            .call(hotsheet_terminals::BrokerRequest::Open {
                id: id.clone(),
                kind,
                command: launch.command,
                args: launch.args,
                cwd: Some(cwd),
                env: launch.env,
            })
            .await
            .map_err(|e| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("broker: {e}"))
            })?;
        let info = match resp {
            hotsheet_terminals::BrokerResponse::Terminal { info } => info,
            other => return Err(broker_err(other)),
        };
        // Tool-in-terminal (HS2-4M67VN): register a live connection + feed its busy from the
        // broker-hosted terminal's inference (polled over the socket), once per fresh terminal.
        if newly_spawned {
            match &req.connect {
                Some(tool) => register_broker_terminal_connection(&state, &id, tool, tb.clone()),
                None => watch_broker_terminal_session_exit(
                    &state,
                    &id,
                    terminal_session_worker_id(&id),
                    tb.clone(),
                ),
            }
        }
        return Ok(Json(broker_info(info)));
    }

    let newly_spawned = state.terminals.get(&term_key(&state, &id)).is_none();
    let spec = hotsheet_terminals::TermSpec {
        kind,
        command: launch.command,
        args: launch.args,
        cwd: Some(std::path::PathBuf::from(cwd)),
        env: launch.env,
        rows: 24,
        cols: 80,
    };
    let term = state
        .terminals
        .get_or_spawn(term_key(&state, &id), spec)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;

    // Tool-in-terminal (HS2-4M67VN): register a live connection + feed its busy from the
    // terminal's inference, once per fresh terminal (not on a reattach).
    if newly_spawned {
        match &req.connect {
            Some(tool) => register_terminal_connection(&state, &id, tool, term.clone()),
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
pub(crate) struct PreparedTerminalLaunch {
    pub(crate) command: String,
    pub(crate) args: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
}

pub(crate) fn terminal_launch(
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
        });
    }
    if let Some(command) = &req.command {
        return Ok(PreparedTerminalLaunch {
            command: command.clone(),
            args: req.args.clone(),
            env: terminal_session_env(state, req, terminal_id, command)?,
        });
    }
    let Some(tool) = req.connect.as_deref() else {
        let command = user_default_shell();
        return Ok(PreparedTerminalLaunch {
            env: terminal_session_env(state, req, terminal_id, &command)?,
            command,
            args: req.args.clone(),
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
    let args = plugin
        .launch_args(req.model.as_deref(), req.effort.as_deref())
        .unwrap_or_default();
    Ok(PreparedTerminalLaunch {
        command: program.to_string_lossy().into_owned(),
        args,
        env,
    })
}

pub(crate) fn user_default_shell() -> String {
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
pub(crate) fn shell_command_args(command: &str, shell: &str) -> Vec<String> {
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
pub(crate) fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) const INHERIT_GLOBAL_SHELL_HISTORY_SETTING: &str =
    "terminal.inherit_global_shell_history";

/// Per-terminal shell history lives under the state's injected machine home, never a
/// direct `HOTSHEET_HOME` read (HS2-FQEESP).
pub(crate) fn terminal_history_home(state: &AppState) -> std::path::PathBuf {
    state.machine_home().join("terminal-history")
}

pub(crate) fn history_key(value: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(value.as_bytes()))[..16].to_string()
}

pub(crate) fn shell_history_environment(
    history_home: &FsPath,
    project: &FsPath,
    terminal_id: &str,
    command: &str,
    inherit: bool,
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
            let original = std::env::var_os("ZDOTDIR")
                .map(std::path::PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from));
            let quoted = |path: &FsPath| path.to_string_lossy().replace('\'', "'\\''");
            for name in [".zshenv", ".zprofile", ".zshrc", ".zlogin", ".zlogout"] {
                let source = original
                    .as_ref()
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
    let mut env = vec![
        ("HOTSHEET_SECRET".to_string(), state.secret.clone()),
        ("HOTSHEET_PROJECT".to_string(), project),
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
    let Some(cwd) = req.cwd.as_deref() else {
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
        .filter(|checkout| cwd.starts_with(&checkout.root))
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
pub(crate) fn terminal_session_env(
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
pub(crate) fn terminal_shell_history_env(
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

pub(crate) fn terminal_shell_history_only_env(
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
pub(crate) fn register_terminal_connection(
    state: &AppState,
    id: &str,
    tool: &str,
    term: std::sync::Arc<hotsheet_terminals::Terminal>,
) {
    let registry = state.drive_registry();
    {
        let mut r = registry.lock_or_recover();
        r.register(hotsheet_aitools::Connection {
            id: id.to_string(),
            project: state.store.root().display().to_string(),
            tool: tool.to_string(),
            role: hotsheet_aitools::Role::Main,
            transport: hotsheet_aitools::Transport::Pty,
            pid: None,
            started_at_ms: now_ms(),
        });
    }
    let conn_id = id.to_string();
    let host = state.host.clone();
    let worker = hotsheet_aitools::session_worker_id(tool, id);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let alive = term.is_alive();
            if !alive {
                {
                    let mut r = registry.lock_or_recover();
                    r.unregister(&conn_id);
                }
                release_session_claims(host, worker).await;
                break;
            }
            {
                let mut r = registry.lock_or_recover();
                match term.activity() {
                    hotsheet_terminals::Activity::Busy => r.note_activity(&conn_id, now_ms()),
                    hotsheet_terminals::Activity::Idle => r.set_idle(&conn_id),
                }
            }
        }
    });
}

/// The broker-mode counterpart of [`register_terminal_connection`] (HS2-ERT00F item 5): the
/// PTY lives in the broker, so the busy feed **polls the broker over the socket** (a `Read`
/// every 500ms) for the terminal's busy/idle instead of reading an in-process `Terminal`. Ends
/// (and unregisters) when the terminal is gone (NotFound / not alive) or the broker is
/// unreachable.
pub(crate) fn register_broker_terminal_connection(
    state: &AppState,
    id: &str,
    tool: &str,
    broker: terminal_broker::TerminalBroker,
) {
    let registry = state.drive_registry();
    {
        let mut r = registry.lock_or_recover();
        r.register(hotsheet_aitools::Connection {
            id: id.to_string(),
            project: state.store.root().display().to_string(),
            tool: tool.to_string(),
            role: hotsheet_aitools::Role::Main,
            transport: hotsheet_aitools::Transport::Pty,
            pid: None,
            started_at_ms: now_ms(),
        });
    }
    let conn_id = id.to_string();
    let host = state.host.clone();
    let worker = hotsheet_aitools::session_worker_id(tool, id);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            match broker
                .call(hotsheet_terminals::BrokerRequest::Read {
                    id: conn_id.clone(),
                })
                .await
            {
                Ok(hotsheet_terminals::BrokerResponse::Read { info, .. }) if info.alive => {
                    let mut r = registry.lock_or_recover();
                    if info.busy {
                        r.note_activity(&conn_id, now_ms());
                    } else {
                        r.set_idle(&conn_id);
                    }
                }
                // Terminal gone (NotFound / exited) or broker unreachable → stop + unregister.
                _ => {
                    {
                        let mut r = registry.lock_or_recover();
                        r.unregister(&conn_id);
                    }
                    release_session_claims(host, worker).await;
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
            Err(error) => tracing::warn!("releasing {worker}'s claims in {id} failed: {error}"),
        }
    }
    if !released.is_empty() {
        tracing::info!("released claims left by {worker}: {}", released.join(", "));
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
pub(crate) struct ForegroundCommand {
    pub(crate) running: bool,
}

impl ForegroundCommand {
    /// Record the latest observation; `true` when a running command just finished. An unknown
    /// observation (`None`) changes nothing.
    pub(crate) fn finished(&mut self, observed: Option<bool>) -> bool {
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
pub(crate) fn watch_terminal_session_exit(
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
pub(crate) fn watch_broker_terminal_session_exit(
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
            match broker
                .call(hotsheet_terminals::BrokerRequest::Read { id: id.clone() })
                .await
            {
                Ok(hotsheet_terminals::BrokerResponse::Read { info, .. }) if info.alive => {
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
                    "releasing {}'s claims in {} failed: {error}",
                    drive.worker_id,
                    drive.store_path.display()
                ),
            }
        }
        if !released.is_empty() {
            tracing::warn!(
                "released claims left by ended chat drives: {}",
                released.join(", ")
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
                register_broker_terminal_connection(state, &info.id, tool, broker.clone());
            }
            _ => watch_broker_terminal_session_exit(state, &info.id, worker, broker.clone()),
        }
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
            tracing::warn!("pruning saved terminal names failed: {error}");
            Vec::new()
        }
    }
}

/// `GET /terminals` — the live terminals (id, alive, busy).
pub(crate) async fn list_terminals(State(state): State<AppState>) -> Json<Vec<TerminalInfo>> {
    Json(with_terminal_names(
        &state,
        live_terminal_infos(&state).await,
    ))
}

/// Every live terminal this server reports, before saved names are applied.
pub(crate) async fn live_terminal_infos(state: &AppState) -> Vec<TerminalInfo> {
    if let Some(tb) = &state.terminal_broker {
        if let Ok(hotsheet_terminals::BrokerResponse::List { terminals }) =
            tb.call(hotsheet_terminals::BrokerRequest::List).await
        {
            return terminals.into_iter().map(broker_info).collect();
        }
        return Vec::new();
    }
    state
        .terminals
        .list()
        .into_iter()
        .filter_map(|key| state.terminals.get(&key).map(|t| term_info(&t, &key.1)))
        .collect()
}

/// Apply the project's saved terminal names (HS2-89FPV1). An unreadable settings file leaves
/// every terminal unnamed rather than failing the terminal list.
pub(crate) fn with_terminal_names(
    state: &AppState,
    mut infos: Vec<TerminalInfo>,
) -> Vec<TerminalInfo> {
    let names = terminal_names::all(&Settings::new(state.store.root())).unwrap_or_default();
    let halts = state.terminal_halts.lock_or_recover().clone();
    let mut questions = state.terminal_questions.lock_or_recover();
    let connections = state.terminal_ai_connections.lock_or_recover().clone();
    let mut last_reports = state.terminal_ai_last_reports.lock_or_recover();
    for info in &mut infos {
        if !info.alive {
            last_reports.remove(&info.id);
        }
        info.name = names.get(&info.id).cloned();
        info.halt = halts.get(&info.id).cloned();
        info.question = if info.alive {
            questions.get(&info.id).cloned()
        } else {
            questions.remove(&info.id);
            None
        };
        info.ai_connection = info
            .alive
            .then(|| connections.get(&info.id).cloned())
            .flatten();
        info.last_hook_report = last_reports.get(&info.id).cloned();
    }
    infos
}

#[derive(Deserialize)]
pub(crate) struct TerminalQuestionReq {
    pub(crate) question: String,
    pub(crate) tool_use_id: String,
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) questions: serde_json::Value,
}

/// A trusted Claude hook reports the question before the terminal waits for its answer.
pub(crate) async fn ask_terminal_question(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TerminalQuestionReq>,
) -> Result<StatusCode, ApiError> {
    if !live_terminal_infos(&state)
        .await
        .iter()
        .any(|info| info.id == id)
    {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "no such terminal"));
    }
    if body.tool_use_id.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "missing tool_use_id",
        ));
    }
    if state
        .terminal_ai_connections
        .lock_or_recover()
        .get(&id)
        .is_some_and(|connection| {
            connection.session_id.is_some() && connection.session_id != body.session_id
        })
    {
        return Ok(StatusCode::NO_CONTENT);
    }
    if forget_terminal_halt(&state, &id) {
        emit_terminal_halted(&state, &id, None);
    }
    let mut questions = state.terminal_questions.lock_or_recover();
    let same = questions.get(&id).is_some_and(|current| {
        current.tool_use_id == body.tool_use_id && current.session_id == body.session_id
    });
    if !same {
        questions.insert(
            id.clone(),
            TerminalQuestion {
                question: body.question.trim().chars().take(500).collect(),
                tool_use_id: body.tool_use_id,
                questions: body.questions,
                session_id: body.session_id,
                answer: None,
                at: OffsetDateTime::now_utc()
                    .format(&Rfc3339)
                    .unwrap_or_default(),
            },
        );
    }
    drop(questions);
    if !same {
        emit_terminal_question(&state, &id);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub(crate) struct TerminalQuestionIdentity {
    pub(crate) tool_use_id: String,
    pub(crate) session_id: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct TerminalQuestionAnswerReq {
    pub(crate) tool_use_id: String,
    pub(crate) at: String,
    #[serde(default)]
    pub(crate) answers: serde_json::Value,
    #[serde(default)]
    pub(crate) native: bool,
}

/// The browser may answer only the exact live tool use in the exact Claude session. A stale
/// notice cannot deliver text to a replacement question or a replacement terminal session.
pub(crate) async fn answer_terminal_question(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TerminalQuestionAnswerReq>,
) -> Result<StatusCode, ApiError> {
    let mut questions = state.terminal_questions.lock_or_recover();
    let current = questions
        .get_mut(&id)
        .filter(|current| current.tool_use_id == body.tool_use_id && current.at == body.at)
        .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "question is no longer active"))?;
    if current.answer.is_some() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "question already answered",
        ));
    }
    if body.native {
        questions.remove(&id);
        drop(questions);
        emit_terminal_question(&state, &id);
        return Ok(StatusCode::NO_CONTENT);
    }
    let expected = current
        .questions
        .as_array()
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "question options unavailable"))?;
    let answers = body
        .answers
        .as_object()
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "answers must be an object"))?;
    if expected.is_empty()
        || expected.len() > 4
        || answers.len() != expected.len()
        || expected.iter().any(|question| {
            question
                .get("question")
                .and_then(serde_json::Value::as_str)
                .and_then(|text| answers.get(text))
                .and_then(serde_json::Value::as_str)
                .is_none_or(|answer| answer.trim().is_empty() || answer.len() > 2000)
        })
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "incomplete question answers",
        ));
    }
    current.answer = Some(body.answers);
    Ok(StatusCode::NO_CONTENT)
}

/// A short HTTP poll keeps the PreToolUse hook responsive to server loss without exposing the
/// answer in the terminal snapshot. Repeated polls return the same answer until resolution.
pub(crate) async fn poll_terminal_question_answer(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<TerminalQuestionIdentity>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let questions = state.terminal_questions.lock_or_recover();
    let current = questions
        .get(&id)
        .filter(|current| {
            current.tool_use_id == query.tool_use_id && current.session_id == query.session_id
        })
        .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "question is no longer active"))?;
    if let Some(answer) = &current.answer {
        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({ "answers": answer })),
        ));
    }
    Ok((StatusCode::NO_CONTENT, Json(serde_json::Value::Null)))
}

#[derive(Default, Deserialize)]
pub(crate) struct TerminalQuestionClearQuery {
    pub(crate) tool_use_id: Option<String>,
    pub(crate) session_id: Option<String>,
}

pub(crate) async fn resolve_terminal_question(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<TerminalQuestionClearQuery>,
) -> StatusCode {
    if forget_terminal_question_for_session(
        &state,
        &id,
        query.tool_use_id.as_deref(),
        query.session_id.as_deref(),
    ) {
        emit_terminal_question(&state, &id);
    }
    StatusCode::NO_CONTENT
}

pub(crate) fn forget_terminal_question(
    state: &AppState,
    id: &str,
    tool_use_id: Option<&str>,
) -> bool {
    forget_terminal_question_for_session(state, id, tool_use_id, None)
}

pub(crate) fn forget_terminal_question_for_session(
    state: &AppState,
    id: &str,
    tool_use_id: Option<&str>,
    session_id: Option<&str>,
) -> bool {
    let mut questions = state.terminal_questions.lock_or_recover();
    if tool_use_id.is_some_and(|expected| {
        questions
            .get(id)
            .is_none_or(|current| current.tool_use_id != expected)
    }) {
        return false;
    }
    if session_id.is_some_and(|expected| {
        questions
            .get(id)
            .is_none_or(|current| current.session_id.as_deref() != Some(expected))
    }) {
        return false;
    }
    questions.remove(id).is_some()
}

pub(crate) fn emit_terminal_question(state: &AppState, id: &str) {
    state.emit(ChangeEvent {
        cursor: None,
        store: String::new(),
        kind: "terminal_question".into(),
        id: id.to_owned(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    });
}

/// Body for `POST /terminals/{id}/halt`, sent by the AI tool's hook adapter in that terminal.
#[derive(Deserialize)]
pub(crate) struct TerminalHaltReq {
    #[serde(default)]
    pub(crate) error_type: Option<String>,
    #[serde(default)]
    pub(crate) message: Option<String>,
    #[serde(default)]
    pub(crate) agent: Option<String>,
}

/// `POST /terminals/{id}/halt` — record that the terminal's AI session stopped on an API error
/// (Claude Code's `StopFailure` hook, HS2-HJ4D1H) and announce it with a `terminal_halted` change
/// event (`message` = the error message) so every client marks the tab. Repeating the same report
/// is not re-announced.
pub(crate) async fn halt_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TerminalHaltReq>,
) -> Result<StatusCode, ApiError> {
    if !live_terminal_infos(&state)
        .await
        .iter()
        .any(|info| info.id == id)
    {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "no such terminal"));
    }
    if forget_terminal_question(&state, &id, None) {
        emit_terminal_question(&state, &id);
    }
    let clean = |value: Option<String>, fallback: &str| {
        let value = value.unwrap_or_default();
        let value = value.trim();
        let value = if value.is_empty() { fallback } else { value };
        value.chars().take(500).collect::<String>()
    };
    let halt = TerminalHalt {
        error_type: clean(body.error_type, "unknown"),
        message: clean(body.message, "The AI session stopped on an error."),
        agent: body
            .agent
            .map(|agent| agent.trim().to_owned())
            .filter(|agent| !agent.is_empty()),
        at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default(),
    };
    let changed = {
        let mut halts = state.terminal_halts.lock_or_recover();
        let same = halts.get(&id).is_some_and(|current| {
            current.error_type == halt.error_type && current.message == halt.message
        });
        // Identical reports belong to the same active episode. Clients dedupe prompts by `at`.
        if !same {
            halts.insert(id.clone(), halt.clone());
        }
        !same
    };
    if changed {
        emit_terminal_halted(&state, &id, Some(halt.message));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Default, Deserialize)]
pub(crate) struct TerminalHaltClearQuery {
    /// A manual clear applies only to the episode the user saw; hooks omit this to clear on resume.
    pub(crate) at: Option<String>,
}

/// `DELETE /terminals/{id}/halt` — the session resumed or the user cleared its stopped state.
/// An optional `at` makes a manual clear safe if a newer failure arrived meanwhile.
pub(crate) async fn clear_terminal_halt(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<TerminalHaltClearQuery>,
) -> StatusCode {
    if forget_terminal_question(&state, &id, None) {
        emit_terminal_question(&state, &id);
    }
    if forget_terminal_halt_if(&state, &id, query.at.as_deref()) {
        emit_terminal_halted(&state, &id, None);
    }
    StatusCode::NO_CONTENT
}

pub(crate) fn forget_terminal_halt(state: &AppState, id: &str) -> bool {
    forget_terminal_halt_if(state, id, None)
}

pub(crate) fn forget_terminal_halt_if(state: &AppState, id: &str, at: Option<&str>) -> bool {
    let mut halts = state.terminal_halts.lock_or_recover();
    if at.is_some_and(|expected| halts.get(id).is_none_or(|halt| halt.at != expected)) {
        return false;
    }
    halts.remove(id).is_some()
}

/// Body for `POST /terminals/{id}/ai-connection`, sent by the AI tool's `SessionStart` hook and
/// reused by an authenticated interactive permission ask.
#[derive(Deserialize)]
pub(crate) struct TerminalAiConnectionReq {
    #[serde(default)]
    pub(crate) agent: Option<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) source: TerminalHookSource,
}

/// `POST /terminals/{id}/ai-connection` — an AI session in the terminal started with Hot Sheet's
/// hooks active (HS2-EV1XK3), announced with a `terminal_ai_connection` change event (`message` =
/// the agent) so every client shows the tab as connected. Repeating the same agent is not
/// re-announced.
pub(crate) async fn connect_terminal_ai(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TerminalAiConnectionReq>,
) -> Result<StatusCode, ApiError> {
    if !live_terminal_infos(&state)
        .await
        .iter()
        .any(|info| info.id == id)
    {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "no such terminal"));
    }
    // A new or resumed AI session in this terminal cannot inherit an earlier session's halt.
    if forget_terminal_halt(&state, &id) {
        emit_terminal_halted(&state, &id, None);
    }
    if forget_terminal_question(&state, &id, None) {
        emit_terminal_question(&state, &id);
    }
    let agent = body
        .agent
        .map(|agent| agent.trim().chars().take(64).collect::<String>())
        .filter(|agent| !agent.is_empty());
    let connection = TerminalAiConnection {
        agent: agent.clone(),
        at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default(),
        session_id: body.session_id.filter(|id| !id.is_empty()),
        source: body.source,
    };
    state
        .terminal_ai_last_reports
        .lock_or_recover()
        .insert(id.clone(), connection.clone());
    let changed = state
        .terminal_ai_connections
        .lock_or_recover()
        .insert(id.clone(), connection)
        .is_none_or(|previous| previous.agent != agent);
    state.emit(ChangeEvent {
        cursor: None,
        store: String::new(),
        kind: "terminal_hook_report".into(),
        id: id.clone(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    });
    if changed {
        emit_terminal_ai_connection(&state, &id, Some(agent.unwrap_or_default()));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /terminals/{id}/ai-connection` — the terminal's AI session ended (`SessionEnd`). A
/// terminal that was not connected is a no-op without an event.
pub(crate) async fn disconnect_terminal_ai(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<TerminalAiConnectionEndQuery>,
) -> StatusCode {
    let Some(removed) = forget_terminal_ai_connection_if(&state, &id, query.session_id.as_deref())
    else {
        return StatusCode::NO_CONTENT;
    };
    if forget_terminal_halt(&state, &id) {
        emit_terminal_halted(&state, &id, None);
    }
    if forget_terminal_question(&state, &id, None) {
        emit_terminal_question(&state, &id);
    }
    if removed {
        emit_terminal_ai_connection(&state, &id, None);
    }
    StatusCode::NO_CONTENT
}

#[derive(Default, Deserialize)]
pub(crate) struct TerminalAiConnectionEndQuery {
    pub(crate) session_id: Option<String>,
}

pub(crate) fn forget_terminal_ai_connection(state: &AppState, id: &str) -> bool {
    forget_terminal_ai_connection_if(state, id, None).unwrap_or(false)
}

/// `None` means this end belongs to a different session; `Some(false)` means no report exists.
pub(crate) fn forget_terminal_ai_connection_if(
    state: &AppState,
    id: &str,
    session_id: Option<&str>,
) -> Option<bool> {
    let mut connections = state.terminal_ai_connections.lock_or_recover();
    if session_id.is_some_and(|expected| {
        connections
            .get(id)
            .and_then(|connection| connection.session_id.as_deref())
            != Some(expected)
    }) {
        return None;
    }
    Some(connections.remove(id).is_some())
}

pub(crate) fn emit_terminal_ai_connection(state: &AppState, id: &str, message: Option<String>) {
    state.emit(ChangeEvent {
        cursor: None,
        store: String::new(),
        kind: "terminal_ai_connection".into(),
        id: id.to_owned(),
        slug: String::new(),
        message,
        activity: None,
        assignment: None,
        turn: None,
    });
}

pub(crate) fn emit_terminal_halted(state: &AppState, id: &str, message: Option<String>) {
    state.emit(ChangeEvent {
        cursor: None,
        store: String::new(),
        kind: "terminal_halted".into(),
        id: id.to_owned(),
        slug: String::new(),
        message,
        activity: None,
        assignment: None,
        turn: None,
    });
}

/// Body for `PUT /terminals/{id}/name`; an absent, null, or blank name clears the rename.
#[derive(Deserialize)]
pub(crate) struct TerminalNameReq {
    #[serde(default)]
    pub(crate) name: Option<String>,
}

/// Response for `PUT /terminals/{id}/name`.
#[derive(Serialize)]
pub(crate) struct TerminalNameResp {
    pub(crate) id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
}

/// `PUT /terminals/{id}/name` `{name}` — save (or clear) a live terminal's tab name in the
/// project's machine-local settings and announce it with a `terminal_renamed` change event
/// (`id` = terminal id, `message` = the new name, absent when cleared) so every connected
/// client retitles the tab without a reload (HS2-89FPV1).
pub(crate) async fn rename_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TerminalNameReq>,
) -> Result<Json<TerminalNameResp>, ApiError> {
    let name = terminal_names::normalize(body.name.as_deref())
        .map_err(|message| ApiError::new(StatusCode::BAD_REQUEST, message))?;
    if !live_terminal_infos(&state)
        .await
        .iter()
        .any(|info| info.id == id)
    {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "no such terminal"));
    }
    let changed = terminal_names::set(&Settings::new(state.store.root()), &id, name.as_deref())
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    if changed {
        emit_terminal_renamed(&state, &id, name.clone());
    }
    Ok(Json(TerminalNameResp { id, name }))
}

pub(crate) fn emit_terminal_renamed(state: &AppState, id: &str, name: Option<String>) {
    state.emit(ChangeEvent {
        cursor: None,
        store: String::new(),
        kind: "terminal_renamed".into(),
        id: id.to_owned(),
        slug: String::new(),
        message: name,
        activity: None,
        assignment: None,
        turn: None,
    });
}

/// A killed terminal's id can be reused by a later terminal, which must not inherit the old
/// tab name.
pub(crate) fn forget_terminal_name(state: &AppState, id: &str) {
    if let Err(error) = terminal_names::set(&Settings::new(state.store.root()), id, None) {
        tracing::warn!("forgetting terminal {id}'s name failed: {error}");
    }
}

/// A terminal's current scrollback + state (`GET /terminals/{id}`). The scrollback is what a
/// re-attaching viewer replays; it's returned as lossy UTF-8 text.
#[derive(Serialize)]
pub(crate) struct TerminalRead {
    #[serde(flatten)]
    pub(crate) info: TerminalInfo,
    pub(crate) scrollback: String,
}

pub(crate) fn named_terminal_info(state: &AppState, info: TerminalInfo) -> TerminalInfo {
    with_terminal_names(state, vec![info])
        .pop()
        .expect("one terminal in, one out")
}

pub(crate) async fn read_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TerminalRead>, ApiError> {
    if let Some(tb) = &state.terminal_broker {
        let resp = tb
            .call(hotsheet_terminals::BrokerRequest::Read { id: id.clone() })
            .await
            .map_err(|e| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("broker: {e}"))
            })?;
        return match resp {
            hotsheet_terminals::BrokerResponse::Read { info, scrollback } => {
                Ok(Json(TerminalRead {
                    info: named_terminal_info(&state, broker_info(info)),
                    scrollback: String::from_utf8_lossy(&scrollback).into_owned(),
                }))
            }
            other => Err(broker_err(other)),
        };
    }
    let term = state
        .terminals
        .get(&term_key(&state, &id))
        .ok_or_else(|| ApiError::not_found(&id))?;
    Ok(Json(TerminalRead {
        info: named_terminal_info(&state, term_info(&term, &id)),
        scrollback: String::from_utf8_lossy(&term.scrollback()).into_owned(),
    }))
}

/// Body for `POST /terminals/{id}/input`.
#[derive(Deserialize)]
pub(crate) struct TerminalInput {
    /// Bytes to write to the PTY (as text — includes any control chars like `\n`).
    pub(crate) data: String,
}

pub(crate) async fn write_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TerminalInput>,
) -> Result<StatusCode, ApiError> {
    if let Some(tb) = &state.terminal_broker {
        let resp = tb
            .call(hotsheet_terminals::BrokerRequest::Input {
                id: id.clone(),
                data: body.data.into_bytes(),
            })
            .await
            .map_err(|e| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("broker: {e}"))
            })?;
        return match resp {
            hotsheet_terminals::BrokerResponse::Ok => Ok(StatusCode::NO_CONTENT),
            other => Err(broker_err(other)),
        };
    }
    let term = state
        .terminals
        .get(&term_key(&state, &id))
        .ok_or_else(|| ApiError::not_found(&id))?;
    term.write(body.data.as_bytes())
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn kill_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if let Some(tb) = &state.terminal_broker {
        let resp = tb
            .call(hotsheet_terminals::BrokerRequest::Kill { id: id.clone() })
            .await
            .map_err(|e| {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("broker: {e}"))
            })?;
        return match resp {
            hotsheet_terminals::BrokerResponse::Ok
            | hotsheet_terminals::BrokerResponse::NotFound => {
                forget_terminal_name(&state, &id);
                if forget_terminal_halt(&state, &id) {
                    emit_terminal_halted(&state, &id, None);
                }
                if forget_terminal_question(&state, &id, None) {
                    emit_terminal_question(&state, &id);
                }
                forget_terminal_ai_connection(&state, &id);
                state.terminal_ai_last_reports.lock_or_recover().remove(&id);
                Ok(StatusCode::NO_CONTENT)
            }
            other => Err(broker_err(other)),
        };
    }
    state
        .terminals
        .kill(&term_key(&state, &id))
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    forget_terminal_name(&state, &id);
    if forget_terminal_halt(&state, &id) {
        emit_terminal_halted(&state, &id, None);
    }
    if forget_terminal_question(&state, &id, None) {
        emit_terminal_question(&state, &id);
    }
    forget_terminal_ai_connection(&state, &id);
    state.terminal_ai_last_reports.lock_or_recover().remove(&id);
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /terminals/{id}/attach?secret=…` — the **live** terminal attach (HS2-XTTTMV): a
/// WebSocket that first replays the scrollback (one binary frame), then streams each new PTY
/// output chunk as a binary frame and forwards any binary/text the viewer sends as PTY input.
/// The socket closes when the terminal's child exits or the viewer disconnects.
pub(crate) async fn attach_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<WsParams>,
    ws: WebSocketUpgrade,
) -> Response {
    if params.secret.as_deref() != Some(state.secret.as_str()) {
        return (StatusCode::UNAUTHORIZED, "missing or invalid secret").into_response();
    }
    // Broker mode: the PTY lives in the detached broker. Confirm the terminal exists (clean
    // 404) before upgrading, then bridge the WebSocket to a streaming broker connection
    // (HS2-ERT00F item 4). The size self-heal on disconnect happens broker-side.
    if let Some(tb) = &state.terminal_broker {
        match tb
            .call(hotsheet_terminals::BrokerRequest::Read { id: id.clone() })
            .await
        {
            Ok(hotsheet_terminals::BrokerResponse::Read { .. }) => {}
            Ok(hotsheet_terminals::BrokerResponse::NotFound) => {
                return (StatusCode::NOT_FOUND, "no such terminal").into_response();
            }
            Ok(other) => return broker_err(other).into_response(),
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("broker: {e}")).into_response();
            }
        }
        let socket_path = tb.socket.clone();
        return ws.on_upgrade(move |socket| broker_attach_loop(socket, socket_path, id));
    }
    let Some(term) = state.terminals.get(&term_key(&state, &id)) else {
        return (StatusCode::NOT_FOUND, "no such terminal").into_response();
    };
    ws.on_upgrade(move |socket| terminal_attach_loop(socket, term))
}

pub(crate) fn default_true() -> bool {
    true
}

/// An inbound control message on the terminal WS (a Text frame). Today: a **size claim**
/// (HS2-BD7Q74). Ordinary Text is raw PTY input so a simple client can type over the same
/// socket; malformed JSON carrying the reserved `resize` member is dropped (Binary is input).
#[derive(Deserialize)]
pub(crate) struct TermControl {
    #[serde(default)]
    pub(crate) resize: Option<ResizeClaim>,
}

/// A viewport's leased size claim (HS2-BD7Q74).
#[derive(Deserialize)]
pub(crate) struct ResizeClaim {
    pub(crate) viewer_id: String,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    #[serde(default)]
    pub(crate) focus: bool,
    #[serde(default = "default_true")]
    pub(crate) visible: bool,
    /// This claim was driven by a genuine user interaction (tap/click/focus/keystroke) rather
    /// than a heartbeat; only interacting claims advance the size-arbiter recency (HS2-3ZBQDG).
    #[serde(default)]
    pub(crate) interacting: bool,
}

pub(crate) enum TerminalText<'a> {
    Resize(ResizeClaim),
    InvalidControl,
    Input(&'a str),
}

pub(crate) fn classify_terminal_text(value: &str) -> TerminalText<'_> {
    if let Ok(TermControl {
        resize: Some(resize),
    }) = serde_json::from_str(value)
    {
        return TerminalText::Resize(resize);
    }
    if serde_json::from_str::<serde_json::Value>(value)
        .ok()
        .and_then(|json| json.get("resize").cloned())
        .is_some()
    {
        return TerminalText::InvalidControl;
    }
    TerminalText::Input(value)
}

/// The size the server chose, pushed to every viewer on attach and whenever it changes.
#[derive(Serialize)]
pub(crate) struct SizeMsg<'a> {
    pub(crate) pty_size: PtySizeMsg,
    pub(crate) driven_by: Option<&'a str>,
}
#[derive(Serialize)]
pub(crate) struct PtySizeMsg {
    pub(crate) cols: u16,
    pub(crate) rows: u16,
}

/// The `{pty_size, driven_by}` text frame a viewer receives on attach and on every change.
pub(crate) fn terminal_size_message(cols: u16, rows: u16, driven_by: Option<&str>) -> Message {
    let msg = SizeMsg {
        pty_size: PtySizeMsg { cols, rows },
        driven_by,
    };
    // Serializing two integers and an optional string cannot fail.
    Message::Text(serde_json::to_string(&msg).unwrap_or_default().into())
}

/// A replay after initial attach replaces the viewer's emulator state rather than appending.
pub(crate) fn terminal_replay_control() -> Message {
    Message::Text(r#"{"terminal_replay":"replace"}"#.into())
}

/// Monotonic-ish wall clock in ms for the size arbiter (real millis; the arbiter is
/// deterministic given it).
pub(crate) fn term_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Drive one attached viewer: replay scrollback, then interleave live output → socket, the
/// arbiter's size decisions → socket, and socket input/claims → PTY, on one task via
/// `select!`. On disconnect the viewport's size claim is dropped so the size self-heals.
pub(crate) async fn terminal_attach_loop(
    mut socket: WebSocket,
    term: std::sync::Arc<hotsheet_terminals::Terminal>,
) {
    use tokio::sync::broadcast::error::RecvError;

    // Snapshot and subscribe under one output boundary so concurrent PTY output lands exactly
    // once in either the replay or the live stream (HS2-5W0V9M).
    let (snapshot, mut rx) = term.subscribe_with_scrollback();
    let mut size_rx = term.subscribe_size();
    let mut my_viewer: Option<String> = None;

    // An empty binary replay is still the attach boundary. Reconnecting browsers use it to
    // atomically clear stale emulator state without blanking while waiting for a payload.
    if socket.send(Message::Binary(snapshot.into())).await.is_err() {
        return;
    }
    // The size channel only reports changes, so a viewer of a stable-size terminal would never
    // learn the PTY grid. Report the applied size right after the replay (HS2-7Y1BQ2); the size
    // receiver is already subscribed, so a concurrent change still arrives after this frame.
    if let Some(current) = term.current_size() {
        if socket
            .send(terminal_size_message(
                current.cols,
                current.rows,
                current.driven_by.as_deref(),
            ))
            .await
            .is_err()
        {
            return;
        }
    }

    loop {
        tokio::select! {
            output = rx.recv() => match output {
                Ok(chunk) => {
                    if socket.send(Message::Binary(chunk.into())).await.is_err() {
                        break; // viewer went away
                    }
                }
                // Fell behind the fan-out buffer — re-sync from a fresh snapshot.
                Err(RecvError::Lagged(_)) => {
                    let (snap, replacement) = term.subscribe_with_scrollback();
                    rx = replacement;
                    if socket.send(terminal_replay_control()).await.is_err() {
                        break;
                    }
                    if socket.send(Message::Binary(snap.into())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Closed) => break, // terminal ended
            },
            size = size_rx.recv() => match size {
                Ok(d) => {
                    if socket.send(terminal_size_message(d.cols, d.rows, d.driven_by.as_deref())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(_)) => {} // a missed size update self-corrects on the next claim
                Err(RecvError::Closed) => break,
            },
            inbound = socket.recv() => match inbound {
                // A write to an exited PTY is dropped; the exit is reported separately.
                Some(Ok(Message::Binary(b))) => { let _ = term.write(&b); }
                Some(Ok(Message::Text(t))) => {
                    match classify_terminal_text(&t) {
                        // A size claim: feed the arbiter (it resizes + broadcasts if the size changed).
                        TerminalText::Resize(r) => {
                            my_viewer = Some(r.viewer_id.clone());
                            let now = term_now_ms();
                            term.claim_size(
                                hotsheet_terminals::ViewportClaim {
                                    viewer_id: r.viewer_id,
                                    cols: r.cols,
                                    rows: r.rows,
                                    focus: r.focus,
                                    visible: r.visible,
                                    interacting: r.interacting,
                                    activity_at_ms: now,
                                },
                                now,
                            );
                        }
                        // A malformed control frame is never terminal input: dropping it avoids
                        // echoing protocol JSON into the user's shell during transient layout.
                        TerminalText::InvalidControl => {}
                        TerminalText::Input(input) => { let _ = term.write(input.as_bytes()); }
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {} // ping/pong handled by axum
                Some(Err(_)) => break,
            },
        }
    }

    // Self-heal: drop this viewport's claim so the PTY size recomputes for the rest.
    if let Some(v) = my_viewer {
        term.drop_viewer(&v, term_now_ms());
    }
}

/// Bridge one attached viewer to a terminal that lives in the **detached broker** (HS2-ERT00F
/// item 4): open a streaming broker connection, then forward broker output/size frames → the
/// WebSocket and the viewer's input/size claims → the broker, on one task via `select!`. The
/// broker replays the scrollback as its first frame(s) and drops the size claim when this
/// connection closes, so the size self-heal holds even across a server restart.
pub(crate) async fn broker_attach_loop(
    mut socket: WebSocket,
    broker_socket: std::path::PathBuf,
    id: String,
) {
    let mut stream = match hotsheet_terminals::BrokerStream::open(&broker_socket, &id).await {
        Ok(s) => s,
        Err(_) => return, // dropping the socket closes it
    };
    let mut received_initial_replay = false;

    loop {
        tokio::select! {
            frame = stream.next() => match frame {
                Ok(Some(f)) => {
                    use hotsheet_terminals::StreamOut as S;
                    match f {
                        S::Scrollback { data } => {
                            if received_initial_replay
                                && socket.send(terminal_replay_control()).await.is_err()
                            {
                                break;
                            }
                            received_initial_replay = true;
                            if socket.send(Message::Binary(data.into())).await.is_err() {
                                break;
                            }
                        }
                        S::Output { data } => {
                            if socket.send(Message::Binary(data.into())).await.is_err() {
                                break;
                            }
                        }
                        S::Size { cols, rows, driven_by } => {
                            if socket.send(terminal_size_message(cols, rows, driven_by.as_deref())).await.is_err() {
                                break;
                            }
                        }
                        // The terminal's gone (or the attach failed) — end the viewer session.
                        S::NotFound | S::Err { .. } => break,
                    }
                }
                Ok(None) | Err(_) => break, // broker closed (terminal ended / broker gone)
            },
            inbound = socket.recv() => match inbound {
                Some(Ok(Message::Binary(b))) => {
                    if stream
                        .send(&hotsheet_terminals::StreamIn::Input { data: b.to_vec() })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Some(Ok(Message::Text(t))) => {
                    let sent = match classify_terminal_text(&t) {
                        // A size claim forwards as a Resize frame the broker feeds to its arbiter.
                        TerminalText::Resize(r) => {
                            stream
                                .send(&hotsheet_terminals::StreamIn::Resize {
                                    viewer_id: r.viewer_id,
                                    cols: r.cols,
                                    rows: r.rows,
                                    focus: r.focus,
                                    visible: r.visible,
                                    interacting: r.interacting,
                                })
                                .await
                        }
                        TerminalText::InvalidControl => continue,
                        TerminalText::Input(input) => {
                            stream
                                .send(&hotsheet_terminals::StreamIn::Input {
                                    data: input.as_bytes().to_vec(),
                                })
                                .await
                        }
                    };
                    if sent.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {} // ping/pong handled by axum
                Some(Err(_)) => break,
            },
        }
    }

    // Dropping `stream` closes the broker connection, so the broker self-heals the size claim;
    // dropping `socket` closes the WebSocket to the viewer.
}

/// Body for `POST /permissions/{id}`: the human's answer.
#[derive(Deserialize)]
pub(crate) struct PermissionAnswer {
    pub(crate) decision: hotsheet_aitools::PermissionDecision,
    /// `once` | `session` | `always` (default `once`).
    #[serde(default = "default_scope")]
    pub(crate) scope: hotsheet_aitools::PermissionScope,
}

pub(crate) fn default_scope() -> hotsheet_aitools::PermissionScope {
    hotsheet_aitools::PermissionScope::Once
}

/// The ack a resolve returns: who it was routed back to + the decision applied.
#[derive(Serialize)]
pub(crate) struct PermissionResolved {
    pub(crate) connection: String,
    pub(crate) decision: hotsheet_aitools::PermissionDecision,
    /// Whether an `Always` rule was persisted durably.
    pub(crate) persisted: bool,
}

/// `POST /permissions/{id}` `{decision, scope}` — answer a pending request. Wakes the
/// blocked tool, and on `always` persists the rule so the answer survives a restart. 404
/// if the id isn't pending (already answered / never existed).
pub(crate) async fn resolve_permission(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Json(body): Json<PermissionAnswer>,
) -> Result<Json<PermissionResolved>, ApiError> {
    let event_decision = match body.decision {
        hotsheet_aitools::PermissionDecision::Allow => "allow",
        hotsheet_aitools::PermissionDecision::Deny => "deny",
    };
    let event_scope = match body.scope {
        hotsheet_aitools::PermissionScope::Once => "once",
        hotsheet_aitools::PermissionScope::Session => "session",
        hotsheet_aitools::PermissionScope::Always => "always",
    };
    let resolved = state
        .permissions
        .resolve(id, body.decision, body.scope)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, format!("no pending request {id}")))?;
    // Persist an `Always` rule so the remembered answer survives a restart.
    let mut persisted = false;
    let rules_path = state
        .permission_rule_paths
        .lock_or_recover()
        .get(&resolved.project)
        .cloned();
    if let (Some(rule), Some(path)) = (&resolved.persisted_rule, rules_path.as_ref()) {
        match hotsheet_aitools::append_permission_rule(path, rule) {
            Ok(()) => persisted = true,
            Err(e) => tracing::warn!("failed to persist permission rule: {e}"),
        }
    }
    // Resolution can happen through another client or transport. Publish a replayable
    // nudge so every notification inbox reconciles the now-absent request into history.
    state.emit(ChangeEvent {
        cursor: None,
        store: resolved.project.clone(),
        kind: "permission_resolved".to_string(),
        id: id.to_string(),
        slug: String::new(),
        message: Some(format!("{event_decision}:{event_scope}")),
        activity: None,
        assignment: None,
        turn: None,
    });
    Ok(Json(PermissionResolved {
        connection: resolved.connection,
        decision: resolved.decision,
        persisted,
    }))
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
    use super::{shell_command_args, shell_history_environment};

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
        let zsh =
            shell_history_environment(home.path(), &project_a, "one", "/bin/zsh", false).unwrap();
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
        if std::path::Path::new("/bin/zsh").is_file() {
            let output = std::process::Command::new("/bin/zsh")
                .args(["-ic", "print -r -- $HISTFILE"])
                .envs(zsh.iter().cloned())
                .output()
                .unwrap();
            let printed = String::from_utf8_lossy(&output.stdout);
            assert_eq!(
                printed.trim(),
                zsh.iter().find(|(key, _)| key == "HISTFILE").unwrap().1
            )
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

#[cfg(test)]
mod terminal_text_tests {
    use super::{TerminalText, classify_terminal_text, terminal_replay_control};
    use axum::extract::ws::Message;

    #[test]
    fn malformed_resize_controls_are_dropped_instead_of_becoming_pty_input() {
        assert!(matches!(
            classify_terminal_text(r#"{"resize":{"viewer_id":"viewer","cols":null,"rows":null}}"#),
            TerminalText::InvalidControl
        ));
        assert!(matches!(
            classify_terminal_text(r#"{"resize":{"viewer_id":"viewer","cols":80,"rows":24}}"#),
            TerminalText::Resize(_)
        ));
        assert!(matches!(
            classify_terminal_text("echo hello\n"),
            TerminalText::Input("echo hello\n")
        ));
    }

    #[test]
    fn lag_resync_has_an_explicit_replacement_control() {
        assert!(matches!(
            terminal_replay_control(),
            Message::Text(value) if value.as_str() == r#"{"terminal_replay":"replace"}"#
        ));
    }
}
