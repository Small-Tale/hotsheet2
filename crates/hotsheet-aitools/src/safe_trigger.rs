//! A **reusable, safety-baked trigger**: resolve a tool, assemble the HS2-103 launch safety
//! ([`launch_safety`]) once, and drive turns against it. Shared by the CLI (`trigger`/`work`)
//! and the server's distributed driving loop (HS2-1TY7GC) so the launch machinery lives in
//! exactly one place. The per-turn output is an injected sink, so a caller renders it however
//! it likes (the CLI to stdout, the server to a quiet/log sink).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::launch_safety;
use crate::registry::ConnectionRegistry;
use crate::{DoneReason, LiveError, LiveTrigger, Role, TurnEvent, run_trigger};

/// A resolved tool + its HS2-103 launch isolation, reusable across turns.
pub struct SafeTrigger {
    plugin: hotsheet_plugins::Plugin,
    /// The project directory the tool runs in (readable so callers can log it).
    pub cwd: PathBuf,
    env: Vec<(String, String)>,
    mcp_config: Option<PathBuf>,
    permission_mode: String,
    // Drive codex via its isolated-home shared daemon (reuse one instance) vs. a fresh
    // app-server process per turn (HS2-B7C66H).
    shared_daemon: bool,
    // A live permission bridge to block approvals on a human (HS2-Q1F6HV); None = auto-
    // approve (the CLI's headless default).
    permission_bridge: Option<std::sync::Arc<crate::permission::SharedPermissionBridge>>,
    // Kept alive so the shim dir survives every turn; dropped when the SafeTrigger is.
    _shim: launch_safety::ShimDir,
    // The throwaway codex CODEX_HOME (app-server tools only), kept alive for every turn.
    _codex_home: Option<launch_safety::IsolatedCodexHome>,
}

/// Resolve the tool, assemble the HS2-103 launch safety, and return a reusable
/// [`SafeTrigger`]. Fails (before launching anything) on the preflight gates.
pub fn prepare_trigger(
    store_path: &Path,
    tool: &str,
    project: Option<PathBuf>,
    mcp_config: Option<PathBuf>,
    permission_mode: Option<String>,
    envs: Vec<String>,
    shared_daemon: bool,
) -> Result<SafeTrigger> {
    let plugin = hotsheet_plugins::find(tool)
        .with_context(|| format!("unknown tool '{tool}' (no such plugin)"))?;
    let cwd = project.unwrap_or_else(|| store_path.to_path_buf());

    // `--env K=V` pairs for the launched tool (e.g. an isolated CODEX_HOME).
    let mut env: Vec<(String, String)> = envs
        .iter()
        .map(|kv| {
            kv.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .with_context(|| format!("--env expects KEY=VALUE, got '{kv}'"))
        })
        .collect::<Result<_>>()?;

    // ---- HS2-103 launch safety (baked in so a bare `trigger`/`work` is safe) ----
    launch_safety::assert_no_hs1(&cwd)?;

    // Codex (`app-server`) reads its MCP servers from `$CODEX_HOME`, so the `--mcp-config`
    // isolation below can't reach it. Instead, unless the caller pinned a `CODEX_HOME`, hand
    // it a throwaway MCP-free home whose only server is the Hot Sheet shim (HS2-YRDQNX) — so
    // a bare `trigger codex` can't load the user's global MCP servers (e.g. an HS1 channel).
    let transport = plugin
        .manifest
        .drive
        .as_ref()
        .map(|d| d.transport.as_str())
        .unwrap_or("");
    let is_app_server = transport == "app-server";
    let codex_home = if is_app_server && !env.iter().any(|(k, _)| k == "CODEX_HOME") {
        let store_abs = store_path.canonicalize().with_context(|| {
            format!(
                "store path does not exist: {} (run `hotsheet-cli init` first)",
                store_path.display()
            )
        })?;
        let command = launch_safety::mcp_command(&plugin.manifest.mcp.command);
        let args = plugin.mcp_args(&store_abs.to_string_lossy());
        let source = launch_safety::default_codex_home();
        let name = &plugin.manifest.mcp.server_name;
        // For the shared daemon, the home must be daemon-ready (packages symlinked, short
        // socket path); otherwise the plain isolated home is enough for a direct app-server.
        let home = if shared_daemon {
            let program = plugin.manifest.drive.as_ref().map(|d| d.program.as_str());
            launch_safety::IsolatedCodexHome::create_for_daemon(
                &source,
                name,
                &command,
                &args,
                program.unwrap_or("codex"),
            )?
        } else {
            launch_safety::IsolatedCodexHome::create(&source, name, &command, &args)?
        };
        env.push((
            "CODEX_HOME".to_string(),
            home.path().to_string_lossy().into_owned(),
        ));
        Some(home)
    } else {
        None
    };

    // Put a `hotsheet` → `hotsheet-cli` shim (and the CLI's own dir) at the front of the
    // launched tool's PATH, so a bare `hotsheet` hits our safe CLI (not an HS1 launcher).
    let exe_dir = launch_safety::exe_dir()?;
    let hotsheet_cli = std::env::current_exe()?;
    let shim = launch_safety::ShimDir::create(&hotsheet_cli)?;
    let base_path = env
        .iter()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());
    let child_path = launch_safety::prepend_path(&[shim.path(), &exe_dir], &base_path);
    launch_safety::assert_hotsheet_resolves(&child_path, shim.path())?;
    env.retain(|(k, _)| k != "PATH");
    env.push(("PATH".to_string(), child_path));

    // MCP isolation: restrict the tool to only the Hot Sheet shim by defaulting
    // `--mcp-config` to the tool's project config (Claude gets `--strict-mcp-config`).
    // This requires the tool to have been set up in the project. Codex (`app-server`)
    // isolates via its throwaway `CODEX_HOME/config.toml` above, not `--mcp-config`, so it
    // needs no project setup here.
    let mcp_config = if is_app_server {
        None
    } else {
        match mcp_config {
            Some(p) => Some(p),
            None => {
                let target = cwd.join(&plugin.manifest.mcp.target);
                if !target.exists() {
                    bail!(
                        "{tool} isn't set up for Hot Sheet in {} (no {}). Run \
                         `hotsheet-cli setup {tool}` there first — trigger needs it for HS2-103 \
                         MCP isolation.",
                        cwd.display(),
                        plugin.manifest.mcp.target
                    );
                }
                Some(target)
            }
        }
    };

    Ok(SafeTrigger {
        plugin,
        cwd,
        env,
        mcp_config,
        // Headless work needs a non-blocking permission mode (channel tools); the real
        // permission bridge round-trip is HS2-9R9YZW / HS2-Q1F6HV.
        permission_mode: permission_mode.unwrap_or_else(|| "acceptEdits".to_string()),
        shared_daemon,
        permission_bridge: None,
        _shim: shim,
        _codex_home: codex_home,
    })
}

impl SafeTrigger {
    /// The plugin id being driven.
    pub fn tool(&self) -> &str {
        self.plugin.id()
    }

    /// Attach a live permission bridge so a driven codex's approvals **block for a human**
    /// (the server route-back) instead of auto-approving (HS2-Q1F6HV). Builder-style.
    pub fn with_permission_bridge(
        mut self,
        bridge: std::sync::Arc<crate::permission::SharedPermissionBridge>,
    ) -> Self {
        self.permission_bridge = Some(bridge);
        self
    }

    /// Drive one turn, streaming the tool's events to `on_event` (including the terminal
    /// `Done`). Each call spawns a fresh process (session-resume continuity is HS2-3C1XK3).
    /// `conn_id` labels the connection in `registry` (busy tracking) — a caller that drives
    /// several tickets should pass a distinct id per ticket.
    pub fn run_turn(
        &self,
        prompt: &str,
        resume: Option<&str>,
        worker: bool,
        conn_id: String,
        registry: &mut ConnectionRegistry,
        on_event: &mut dyn FnMut(&TurnEvent),
    ) -> Result<DoneReason> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let t = LiveTrigger {
            cwd: self.cwd.clone(),
            prompt: prompt.to_string(),
            role: if worker { Role::Worker } else { Role::Main },
            conn_id,
            resume: resume.map(str::to_string),
            mcp_config: self.mcp_config.clone(),
            permission_mode: Some(self.permission_mode.clone()),
            env: self.env.clone(),
            shared_daemon: self.shared_daemon,
            permission_bridge: self.permission_bridge.clone(),
            now_ms,
        };
        run_trigger(&self.plugin, &t, registry, on_event).map_err(|e| match e {
            LiveError::NotDrivable(id) => {
                anyhow::anyhow!(
                    "'{id}' is not drivable (no [drive], or its transport isn't built yet)"
                )
            }
            other => anyhow::Error::new(other),
        })
    }
}
