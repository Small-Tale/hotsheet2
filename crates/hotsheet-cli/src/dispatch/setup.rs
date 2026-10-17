//! AI tool setup, settings, plugins, permission hooks, launch, and driven turns.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use hotsheet_cli::git_init;
use hotsheet_model::Ulid;
use hotsheet_ticketing::{FsStore, SortKey, StoreMetadata, TicketQuery, ops};

use crate::{
    Cmd, ai_settings_target, binary_on_path, confirm, discovered_ai_tools, effective_ai_defaults,
    now_ts, settings_for_cli, stored_ai_defaults, urlencoding_component,
};

use super::{Ctx, misrouted};

pub(super) fn run(cmd: Cmd, ctx: &Ctx) -> Result<()> {
    let path = &ctx.path;
    let cwd = &ctx.cwd;
    match cmd {
        Cmd::Setup {
            tool,
            detect,
            refresh,
            project,
            json,
        } => cmd_setup(path, tool, detect, refresh, project, json),
        Cmd::Plugin { cmd } => cmd_plugin(cmd),
        Cmd::AiTools { json } => cmd_ai_tools(json),
        Cmd::AiSettings { cmd } => cmd_ai_settings(path, cwd, cmd),
        Cmd::Settings { cmd } => cmd_settings(path, cwd, cmd),
        Cmd::PermissionHook { agent } => cmd_permission_hook(agent.as_deref()),
        Cmd::HookDiagnose { json } => cmd_hook_diagnose(json),
        Cmd::Launch {
            tool,
            project,
            ticket_store,
            create_ticket_store,
            args,
        } => cmd_launch(
            path,
            cwd,
            &tool,
            project,
            ticket_store,
            create_ticket_store,
            args,
        ),
        Cmd::Trigger(args) => cmd_trigger(path, args),
        Cmd::Work(args) => cmd_work(path, args),
        _ => misrouted("setup"),
    }
}

pub(crate) fn cmd_trigger(store_path: &Path, args: TriggerArgs) -> Result<()> {
    let TriggerArgs {
        tool,
        prompt,
        project,
        resume,
        mcp_config,
        permission_mode,
        envs,
        worker,
        shared_daemon,
    } = args;
    let tool = tool.as_str();
    use hotsheet_aitools::{ConnectionRegistry, DoneReason, prepare_trigger};

    let safe = prepare_trigger(
        store_path,
        tool,
        project,
        mcp_config,
        permission_mode,
        envs,
        shared_daemon,
    )?;
    eprintln!("▶ driving {tool} in {} …", safe.cwd.display());
    let mut registry = ConnectionRegistry::new(30_000);
    let done = safe.run_turn(
        &prompt.unwrap_or_else(|| DEFAULT_TRIGGER_PROMPT.to_string()),
        // A one-off `--resume` from the command line has no recorded baseline, so its cost
        // includes the resumed session's earlier turns (HS2-4JPDJA).
        resume.as_deref().map(hotsheet_aitools::ResumeSession::id),
        worker,
        format!("cli-{}", std::process::id()),
        &mut registry,
        &mut stream_to_stdout,
    )?;
    println!();
    if let Some(sid) = &done.session_id {
        eprintln!("  session: {sid}");
    }
    match done.reason {
        DoneReason::Completed => {
            eprintln!("✔ {tool} turn completed");
            Ok(())
        }
        DoneReason::Failed { exit_code, message } => bail!(
            "{tool} turn failed (exit {exit_code}){}",
            message
                .map(|message| format!(": {message}"))
                .unwrap_or_default()
        ),
        DoneReason::Interrupted => bail!("{tool} turn interrupted"),
    }
}

/// `hotsheet-cli work <tool>`: drive the tool one turn at a time until Up Next is
/// drained, a turn cap is hit, or the queue stops changing (thrash guard). The
/// north-star headless loop (HS2-118), reusing `trigger`'s HS2-103 launch safety.
pub(crate) fn cmd_work(store_path: &Path, args: WorkArgs) -> Result<()> {
    let WorkArgs {
        tool,
        project,
        max,
        max_stall,
        worker,
        shared_daemon,
    } = args;
    let tool = tool.as_str();
    use hotsheet_aitools::{ConnectionRegistry, DoneReason, prepare_trigger};
    use hotsheet_cli::workloop::{Stall, queue_signature};

    let store = FsStore::open(store_path)?;
    let up_next_query = || TicketQuery {
        up_next_only: true,
        open_only: true,
        sort: SortKey::Priority,
        ..Default::default()
    };

    // Nothing to do? Exit before requiring setup / building the launch machinery.
    if ops::query(&store, &up_next_query())?.is_empty() {
        eprintln!("✔ Nothing Up Next — nothing to do.");
        return Ok(());
    }

    // The loop is the shared daemon's best case: one isolated CODEX_HOME + one codex
    // instance reused across every turn, torn down when this SafeTrigger drops (HS2-9M6T68).
    let safe = prepare_trigger(store_path, tool, project, None, None, vec![], shared_daemon)?;
    let mut registry = ConnectionRegistry::new(30_000);
    let mut stall = Stall::default();
    let mut completed = 0u32;
    // The tool's session/thread id, captured from turn 1 and passed as `resume` on every
    // subsequent turn so one session carries context across the loop (HS2-3C1XK3).
    let mut session: Option<String> = None;
    // The session's cumulative cost after the previous turn, so each resumed turn reports
    // only its own cost (HS2-4JPDJA).
    let mut session_cost: Option<f64> = None;
    for turn in 1..=max {
        let before = ops::query(&store, &up_next_query())?;
        if before.is_empty() {
            eprintln!(
                "✔ Up Next drained after {} turn(s) ({completed} completed).",
                turn - 1
            );
            return Ok(());
        }
        let top = &before[0];
        eprintln!(
            "── turn {turn}/{max}: {} — {} ({} up next) ──",
            top.slug,
            top.title,
            before.len()
        );

        let done = safe.run_turn(
            DEFAULT_TRIGGER_PROMPT_LOOP,
            session
                .as_deref()
                .map(|session_id| hotsheet_aitools::ResumeSession {
                    session_id,
                    cost_baseline_usd: session_cost,
                }),
            worker,
            format!("cli-{}", std::process::id()),
            &mut registry,
            &mut stream_to_stdout,
        )?;
        println!();
        // Remember the session so the next turn resumes it (first non-None wins).
        if session.is_none() {
            session = done.session_id.clone();
        }
        if done.session_id == session && done.session_cost_usd.is_some() {
            session_cost = done.session_cost_usd;
        }
        match done.reason {
            DoneReason::Completed => {}
            DoneReason::Failed { exit_code, message } => eprintln!(
                "⚠ turn {turn} failed (exit {exit_code}){}",
                message
                    .map(|message| format!(": {message}"))
                    .unwrap_or_default()
            ),
            DoneReason::Interrupted => bail!("interrupted during turn {turn}"),
        }

        // Progress = the Up Next queue changed (a ticket left it, or any of them was
        // edited). No change across a whole turn trips the thrash guard.
        let after = ops::query(&store, &up_next_query())?;
        let progressed = queue_signature(&before) != queue_signature(&after);
        if after.len() < before.len() {
            completed += (before.len() - after.len()) as u32;
        }
        let streak = stall.record(progressed);
        if streak >= max_stall {
            bail!(
                "no progress for {max_stall} turns (top ticket {} — {}); stopping. \
                 The tool may be stuck — check it, then re-run.",
                top.slug,
                top.title
            );
        }
    }
    let remaining = ops::query(&store, &up_next_query())?.len();
    bail!("reached --max {max} turns with {remaining} ticket(s) still Up Next");
}

/// Native permission hook: stdin hook JSON → ask the running server → stdout decision.
/// Interactive PermissionRequest (Claude or Codex) and marked Claude headless PreToolUse
/// events use their respective response schemas. Any error emits nothing so the provider's
/// native flow remains authoritative, and the command always exits 0 so a hook cannot wedge it.
pub(crate) fn cmd_permission_hook(installed_agent: Option<&str>) -> Result<()> {
    use hotsheet_cli::permission_hook::{
        PermissionHookEvent, decision_from_server, hook_connection, hook_decision_json,
        hook_tool_action, permission_hook_event, permission_request_decision_json,
        permission_terminal_id, should_bridge_permission,
    };
    let input: serde_json::Value =
        serde_json::from_reader(std::io::stdin()).unwrap_or(serde_json::Value::Null);

    // Session lifecycle events report a halted or resumed AI session (HS2-HJ4D1H), and a session
    // that started or ended with these hooks active (HS2-EV1XK3), to the Hot Sheet terminal this
    // hook runs in. They never print a decision or context, and outside a Hot Sheet
    // terminal or without a reachable server they do nothing.
    if let Some(session) = hotsheet_cli::permission_hook::session_hook_event(&input) {
        let terminal = std::env::var("HOTSHEET_TERMINAL_ID").unwrap_or_default();
        if let (false, Some((url, secret))) = (terminal.is_empty(), hook_server_route()) {
            let env_agent = std::env::var("HOTSHEET_AGENT").ok();
            let agent = installed_agent.or(env_agent.as_deref());
            let session_id = input.get("session_id").and_then(serde_json::Value::as_str);
            if report_terminal_session(&url, &secret, &terminal, &session, agent, session_id)
                .is_ok()
            {
                if let hotsheet_cli::permission_hook::SessionHookEvent::QuestionAsked {
                    tool_use_id,
                    ..
                } = &session
                {
                    if let Some(answers) = wait_for_terminal_question_answer(
                        &url,
                        &secret,
                        &terminal,
                        tool_use_id,
                        session_id,
                    ) {
                        if let Some(output) = hotsheet_cli::permission_hook::question_answer_json(
                            &input["tool_input"],
                            &answers,
                        ) {
                            println!("{output}");
                        }
                    }
                }
            }
        }
        return Ok(());
    }

    let event = permission_hook_event(&input);
    let headless_pre_tool = event == PermissionHookEvent::PreToolUse
        && std::env::var("HOTSHEET_CLAUDE_PRETOOLUSE").as_deref() == Ok("1");
    // In an interactive Claude terminal, PreToolUse observes every harmless operation
    // before Claude evaluates its own rules. Leave those calls alone and wait for the
    // PermissionRequest event that means Claude would actually show a dialog.
    if !should_bridge_permission(event, headless_pre_tool) {
        return Ok(());
    }

    let project = std::env::var("HOTSHEET_PROJECT").unwrap_or_default();
    let route = hook_server_route();
    let terminal_env = std::env::var("HOTSHEET_TERMINAL_ID").ok();
    let terminal = permission_terminal_id(event, terminal_env.as_deref());
    let Some(decision) = (match route {
        // Governed by a Hot Sheet server: raise a blocking request and honor the answer.
        Some((url, secret)) => {
            let (tool, action) = hook_tool_action(&input);
            let connection = hook_connection(&input);
            let env_agent = std::env::var("HOTSHEET_AGENT").ok();
            let agent = installed_agent.or(env_agent.as_deref());
            let request = serde_json::json!({
                "project": project, "connection": connection, "tool": tool, "action": action,
                "agent": agent, "terminal_id": terminal,
                "session_id": input.get("session_id").and_then(serde_json::Value::as_str),
            });
            match ask_server(&url, &secret, &request) {
                Ok(reply) => Some(decision_from_server(&reply)),
                // Server unreachable / error → emit nothing and preserve Claude's native flow.
                Err(_) => None,
            }
        }
        // Not a Hot Sheet-governed run → emit nothing and preserve Claude's native flow.
        None => None,
    }) else {
        return Ok(());
    };
    let output = match event {
        PermissionHookEvent::PermissionRequest => permission_request_decision_json(decision),
        PermissionHookEvent::PreToolUse => Some(hook_decision_json(decision)),
        PermissionHookEvent::Other => None,
    };
    if let Some(output) = output {
        println!("{output}");
    }
    Ok(())
}

/// Set up and replace this process with a hook-capable interactive AI tool. Because the
/// child inherits the ordinary terminal, this behaves like invoking the tool directly.
pub(crate) fn cmd_launch(
    store: &Path,
    cwd: &Path,
    tool: &str,
    project: Option<PathBuf>,
    ticket_store: Option<PathBuf>,
    create_ticket_store: Option<PathBuf>,
    args: Vec<String>,
) -> Result<()> {
    let launch_dir = project.unwrap_or_else(|| cwd.to_path_buf());
    let launch_dir = launch_dir
        .canonicalize()
        .with_context(|| format!("project path does not exist: {}", launch_dir.display()))?;
    let project = hotsheet_cli::launch_project_root(&launch_dir);
    let registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(
        hotsheet_plugins::hotsheet_home().join("checkouts.json"),
    );
    let explicit = ticket_store.or_else(|| (store != Path::new(".")).then(|| store.to_path_buf()));
    let selected = if let Some(path) = explicit {
        FsStore::open(&path)
            .with_context(|| format!("{} is not a Hot Sheet ticket store", path.display()))?;
        path.canonicalize().unwrap_or(path)
    } else if let Some(path) = create_ticket_store {
        FsStore::init(&path, &StoreMetadata::new("HS"))?;
        git_init(&path);
        path.canonicalize().unwrap_or(path)
    } else {
        let discovery = hotsheet_cli::discover_launch_sources(&launch_dir, &registry)?;
        if let Some(path) = discovery.selected {
            path
        } else if discovery.candidates.len() == 1 {
            discovery.candidates[0].clone()
        } else if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            choose_launch_source(&project, &discovery.candidates)?
        } else if discovery.candidates.is_empty() {
            bail!(
                "no ticket source is associated with {}; pass --ticket-store <path> or --create-ticket-store <path>",
                project.display()
            )
        } else {
            bail!(
                "multiple ticket sources match {}: {}; pass --ticket-store <path>",
                project.display(),
                discovery
                    .candidates
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    };
    let source = hotsheet_ticketing::checkouts::TicketSource::git(&selected);
    match registry.resolve(project.to_string_lossy().as_ref()) {
        Ok(_) => {
            registry.add_source(project.to_string_lossy().as_ref(), source.clone(), true)?;
        }
        Err(hotsheet_ticketing::checkouts::CheckoutError::NotFound(_)) => {
            registry.register_sources(
                &project,
                None,
                None,
                vec![source],
                Some(hotsheet_ticketing::checkouts::TicketSource::git(&selected).connection_id),
            )?;
        }
        Err(error) => return Err(error.into()),
    }
    hotsheet_cli::setup::run_setup(&selected, &project, Some(tool), false)?;
    let launch = hotsheet_cli::external_launch::prepare(
        &selected,
        tool,
        args,
        &hotsheet_plugins::hotsheet_home(),
    )?;
    let program = hotsheet_aitools::launch_safety::resolve_program(&launch.program)?;
    let permission_project = FsStore::open(&selected)
        .map(|store| store.root().display().to_string())
        .unwrap_or_else(|_| selected.display().to_string());
    // The session's worker id: the instructions tell the AI to claim with it, and its claims
    // are released when the tool exits (HS2-1VAW1C).
    let worker = hotsheet_cli::launch_worker_id(tool, &Ulid::new());
    let mut command = hotsheet_ticketing::git::launch(&program);
    command
        .args(&launch.args)
        .current_dir(&launch_dir)
        .env("HOTSHEET_SERVER", &launch.server.url)
        .env("HOTSHEET_SECRET", &launch.server.secret)
        .env("HOTSHEET_PROJECT", permission_project)
        .env("HOTSHEET_WORKER_ID", &worker)
        // The launched tool is an AI session: its mutations act as `ai` (HS2-RD4M29).
        .envs(hotsheet_aitools::ai_session_actor_env(&worker));
    let mut child = command
        .spawn()
        .with_context(|| format!("launching {}", program.display()))?;
    // The tool owns the terminal: its interrupt, quit and hang-up signals are for it. Survive
    // them here (after the spawn, so the tool keeps default dispositions) to release its claims
    // once it exits, including when the terminal window closes.
    #[cfg(unix)]
    // SAFETY: setting a signal disposition to SIG_IGN has no handler to race with.
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        libc::signal(libc::SIGQUIT, libc::SIG_IGN);
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }
    let status = child.wait()?;
    if let Ok(store) = FsStore::open(&selected) {
        match ops::release_worker(&store, now_ts(), &worker) {
            Ok(released) if !released.is_empty() => eprintln!(
                "Released claims left by {tool}: {}",
                released
                    .iter()
                    .map(|ticket| ticket.slug.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ok(_) => {}
            Err(error) => eprintln!("could not release {worker}'s claims: {error}"),
        }
    }
    std::process::exit(hotsheet_cli::exit_code_of(status))
}

/// Probe the same authenticated server route that permission hooks resolve, without
/// posting a SessionStart or permission request. MCP availability is independent.
pub(crate) fn cmd_hook_diagnose(json: bool) -> Result<()> {
    let terminal_id = std::env::var("HOTSHEET_TERMINAL_ID")
        .ok()
        .filter(|id| !id.is_empty());
    let mut report = serde_json::json!({
        "mcp": "not_tested",
        "permission_bridge": "unconfigured",
        "terminal_id": terminal_id.clone(),
        "terminal_hook": "unknown",
        "last_hook_report": null,
    });
    if let Some((url, secret)) = hook_server_route() {
        let endpoint = format!("{}/permissions/bridge-probe", url.trim_end_matches('/'));
        let probe = hook_get_json(&endpoint, &secret);
        match probe {
            Ok(value)
                if value
                    .get("bridge_reachable")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true) =>
            {
                report["permission_bridge"] = "reachable".into();
                if let Some(id) = terminal_id.as_deref() {
                    let terminals =
                        hook_get_json(&format!("{}/terminals", url.trim_end_matches('/')), &secret);
                    match terminals {
                        Ok(terminals) => {
                            if let Some(terminal) = terminals
                                .as_array()
                                .and_then(|items| items.iter().find(|item| item["id"] == id))
                            {
                                report["terminal_hook"] =
                                    if terminal.get("ai_connection").is_some() {
                                        "active"
                                    } else {
                                        "not_active"
                                    }
                                    .into();
                                report["last_hook_report"] = terminal
                                    .get("last_hook_report")
                                    .cloned()
                                    .unwrap_or(serde_json::Value::Null);
                            } else {
                                report["terminal_hook"] = "terminal_not_found".into();
                            }
                        }
                        Err(_) => report["terminal_hook"] = "terminal_read_failed".into(),
                    }
                }
            }
            Ok(_) | Err(_) => report["permission_bridge"] = "unreachable".into(),
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("MCP: not tested (MCP connectivity does not prove terminal hooks are active).");
        println!(
            "Permission bridge: {}",
            report["permission_bridge"].as_str().unwrap_or("unknown")
        );
        println!(
            "Terminal hook: {}",
            report["terminal_hook"].as_str().unwrap_or("unknown")
        );
        if !report["last_hook_report"].is_null() {
            println!(
                "Last trusted hook report: {} at {}",
                report["last_hook_report"]["source"]
                    .as_str()
                    .unwrap_or("unknown"),
                report["last_hook_report"]["at"]
                    .as_str()
                    .unwrap_or("unknown")
            );
        }
        if report["terminal_hook"].as_str() != Some("active") {
            println!(
                "For Codex, run /hooks to review and trust Hot Sheet hooks, then restart the session. A successful bridge probe alone does not connect it."
            );
        }
    }
    Ok(())
}

pub(crate) fn cmd_setup(
    store: &Path,
    tool: Option<String>,
    detect: bool,
    refresh: bool,
    project: Option<PathBuf>,
    json: bool,
) -> Result<()> {
    let project_dir = project.unwrap_or_else(|| store.to_path_buf());
    // Setup also makes the checkout discoverable to server/MCP consumers. This records
    // context; generated MCP config still points directly at the store for headless use.
    let repository = hotsheet_ticketing::git::command()
        .args([
            "-C",
            project_dir.to_str().unwrap_or("."),
            "config",
            "--get",
            "remote.origin.url",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty());
    hotsheet_ticketing::checkouts::CheckoutRegistry::new(
        hotsheet_plugins::hotsheet_home().join("checkouts.json"),
    )
    .open_git_store(&project_dir, None, repository, store)?;
    let (reports, removed) = if refresh {
        let report = hotsheet_cli::setup::refresh_setup(store, &project_dir)?;
        if report.is_empty() && !json {
            println!("Setup is current; no applicable AI-tool integrations found.");
        }
        (report.set_up, report.removed)
    } else {
        (
            hotsheet_cli::run_setup(store, &project_dir, tool.as_deref(), detect)?,
            Vec::new(),
        )
    };
    if json {
        println!(
            "{}",
            serde_json::to_string(&hotsheet_plugins::setup::RefreshReport {
                set_up: reports,
                removed
            })?
        );
        return Ok(());
    }
    for r in &reports {
        println!("Set up {} in {}:", r.tool, project_dir.display());
        for w in &r.wrote {
            println!("  wrote {w}");
        }
        for notice in &r.notices {
            println!("  Notice: {}", notice.message());
        }
    }
    // Disabled tools whose managed artifacts refresh removed (HS2-CAM9J5).
    for r in &removed {
        println!(
            "Removed disabled {} setup in {}:",
            r.tool,
            project_dir.display()
        );
        for path in &r.removed {
            println!("  removed {path}");
        }
        for path in &r.edited {
            println!("  edited {path}");
        }
    }
    Ok(())
}

pub(crate) fn cmd_plugin(cmd: PluginCmd) -> Result<()> {
    use hotsheet_plugins::{PluginSource, all_plugins, default_dirs, machine_plugins_dir};
    match cmd {
        PluginCmd::List => {
            for p in all_plugins(&default_dirs()) {
                let src = match &p.source {
                    PluginSource::BuiltIn => "built-in".to_string(),
                    PluginSource::Disk(path) => path.display().to_string(),
                };
                let detected = p
                    .manifest
                    .detection
                    .binaries
                    .iter()
                    .any(|b| binary_on_path(b));
                println!(
                    "{:<10} {:<16} {:<9} {}",
                    p.id(),
                    p.manifest.product_name,
                    if detected { "detected" } else { "-" },
                    src
                );
            }
        }
        PluginCmd::Info { id } => {
            let p =
                hotsheet_plugins::find(&id).with_context(|| format!("unknown plugin '{id}'"))?;
            println!("{}", hotsheet_cli::plugin::describe(&p));
            let issues = hotsheet_cli::plugin::verify(&p);
            if issues.is_empty() {
                println!("  verify:     ok");
            } else {
                for i in &issues {
                    println!("  verify:     ! {i}");
                }
            }
        }
        PluginCmd::Verify { id } => {
            let p =
                hotsheet_plugins::find(&id).with_context(|| format!("unknown plugin '{id}'"))?;
            let issues = hotsheet_cli::plugin::verify(&p);
            if issues.is_empty() {
                println!("Plugin '{id}' passed verification.");
            } else {
                for i in &issues {
                    eprintln!("  ! {i}");
                }
                bail!(
                    "plugin '{id}' failed verification ({} issue(s))",
                    issues.len()
                );
            }
        }
        PluginCmd::Install { dir, yes } => {
            // Trust gate: disclose what the plugin writes + launches before installing.
            let p = hotsheet_plugins::Plugin::from_fs_dir(&dir)
                .with_context(|| format!("not a valid plugin directory: {}", dir.display()))?;
            println!("About to install this plugin:");
            println!("{}", hotsheet_cli::plugin::describe(&p));
            let issues = hotsheet_cli::plugin::verify(&p);
            if !issues.is_empty() {
                for i in &issues {
                    eprintln!("  ! {i}");
                }
                bail!("refusing to install: plugin failed verification");
            }
            if !yes && !confirm("Install this plugin? [y/N] ")? {
                println!("Aborted.");
                return Ok(());
            }
            let dest = machine_plugins_dir();
            let id = hotsheet_cli::plugin::install(&dir, &dest)?;
            println!("Installed plugin '{id}' into {}", dest.join(&id).display());
            eprintln!(
                "note: an installed plugin's code is not yet sandboxed (behavioral boundary: HS2-93)"
            );
        }
        PluginCmd::Remove { id } => {
            if hotsheet_cli::plugin::remove(&id, &machine_plugins_dir())? {
                println!("Removed plugin '{id}'");
            } else {
                println!("No installed plugin '{id}'");
            }
        }
    }
    Ok(())
}

pub(crate) fn cmd_ai_tools(json: bool) -> Result<()> {
    let tools = discovered_ai_tools();
    if json {
        println!("{}", serde_json::to_string(&tools)?);
    } else {
        for tool in tools {
            println!("{}\t{}", tool.id, tool.display_name);
        }
    }
    Ok(())
}

pub(crate) fn cmd_ai_settings(store: &Path, cwd: &Path, cmd: AiSettingsCmd) -> Result<()> {
    match cmd {
        AiSettingsCmd::Get { json, global } => {
            let defaults = effective_ai_defaults(store, cwd, global)?;
            if json {
                println!("{}", serde_json::to_string(&defaults)?);
            } else {
                println!("{}", defaults.tool);
                if let Some(model) = defaults.model {
                    println!("model\t{model}");
                }
                if let Some(effort) = defaults.effort {
                    println!("effort\t{effort}");
                }
                if let Some(mode) = defaults.permission_mode {
                    println!("permission_mode\t{mode}");
                }
            }
        }
        AiSettingsCmd::Set {
            tool,
            model,
            effort,
            permission_mode,
            global,
        } => {
            // Keep every other provider's choices; only the default provider changes.
            let current = effective_ai_defaults(store, cwd, global).ok();
            let defaults = hotsheet_plugins::AiToolDefaults {
                tool,
                model,
                effort,
                permission_mode,
                providers: current.map(|current| current.providers).unwrap_or_default(),
            };
            let saved = save_ai_defaults(store, cwd, global, &defaults)?;
            println!("{}", serde_json::to_string(&saved)?);
        }
        AiSettingsCmd::SetProvider {
            tool,
            model,
            effort,
            permission_mode,
            global,
        } => {
            let mut defaults = effective_ai_defaults(store, cwd, global)?;
            let entry = hotsheet_plugins::AiProviderDefaults {
                model,
                effort,
                permission_mode,
            };
            if defaults.tool == tool {
                defaults.model = entry.model.clone();
                defaults.effort = entry.effort.clone();
                defaults.permission_mode = entry.permission_mode.clone();
            }
            defaults.providers.insert(tool, entry);
            let saved = save_ai_defaults(store, cwd, global, &defaults)?;
            println!("{}", serde_json::to_string(&saved)?);
        }
    }
    Ok(())
}

pub(crate) fn cmd_settings(store: &Path, cwd: &Path, cmd: SettingsCmd) -> Result<()> {
    use hotsheet_ticketing::Scope;
    let parse_scope = |s: &Option<String>| -> Result<Option<Scope>> {
        match s.as_deref() {
            None => Ok(None),
            Some("global") => Ok(Some(Scope::Global)),
            Some("shared") => Ok(Some(Scope::Shared)),
            Some("local") => Ok(Some(Scope::Local)),
            Some(other) => bail!("invalid scope '{other}' (global|shared|local)"),
        }
    };
    let settings = settings_for_cli(store, cwd);
    match cmd {
        SettingsCmd::Get { key, scope } => {
            let value = match parse_scope(&scope)? {
                Some(s) => settings.get(&key, s)?,
                None => settings.get_effective(&key)?,
            };
            match value {
                Some(v) => println!("{}", render_value(&v)),
                None => bail!("no setting '{key}'"),
            }
        }
        SettingsCmd::Set { key, value, scope } => {
            // Terminal memory limits are machine-wide (HS2-Y5DYC0): they default to the global
            // scope, refuse a project scope, and are range-checked like the Settings UI.
            let terminal_limit = hotsheet_ticketing::terminal_limits::is_terminal_limit_key(&key);
            let scope = match (parse_scope(&scope)?, terminal_limit) {
                (Some(Scope::Global) | None, true) => Scope::Global,
                (Some(_), true) => bail!("{key} is machine-wide; use --scope global"),
                (scope, false) => scope.unwrap_or(Scope::Shared),
            };
            if terminal_limit {
                hotsheet_ticketing::terminal_limits::parse_terminal_limit(&key, &value)?;
            }
            // Parse as JSON (numbers/bools/arrays/objects), else store the raw string.
            let parsed = serde_json::from_str(&value).unwrap_or(serde_json::Value::String(value));
            settings.set(&key, parsed, scope)?;
            let where_ = match scope {
                Scope::Global => "global (machine-wide)",
                Scope::Shared => "shared (committed)",
                Scope::Local => "local (gitignored)",
            };
            println!("Set {key} in {where_}");
        }
        SettingsCmd::List { scope } => {
            let map = match parse_scope(&scope)? {
                Some(s) => settings.map(s)?,
                None => settings.effective()?,
            };
            if map.is_empty() {
                println!("(no settings)");
            }
            for (k, v) in &map {
                println!("{k} = {}", render_value(v));
            }
        }
    }
    Ok(())
}

#[derive(Args)]
pub(crate) struct TriggerArgs {
    /// The tool to drive (e.g. `claude`, `codex`).
    tool: String,
    /// The turn content. Defaults to a "work the top Up Next ticket" prompt.
    #[arg(long)]
    prompt: Option<String>,
    /// Project directory the tool runs in (defaults to the store path).
    #[arg(long)]
    project: Option<PathBuf>,
    /// Resume a prior session id (channel tools).
    #[arg(long)]
    resume: Option<String>,
    /// Restrict a channel tool to only this MCP config (`--strict-mcp-config`), so it
    /// can't reach anything but the Hot Sheet shim (HS2-103 isolation).
    #[arg(long)]
    mcp_config: Option<PathBuf>,
    /// Claude permission mode for headless work (e.g. `acceptEdits`,
    /// `bypassPermissions`). Defaults to `acceptEdits`.
    #[arg(long)]
    permission_mode: Option<String>,
    /// Set an env var for the launched tool (repeatable): `--env CODEX_HOME=/path`.
    #[arg(long = "env")]
    envs: Vec<String>,
    /// Register the connection as a self-claim worker rather than the main session.
    #[arg(long)]
    worker: bool,
    /// Codex only: drive the shared app-server **daemon** for the (isolated) CODEX_HOME
    /// — reuse one codex instance across turns instead of a fresh process per turn
    /// (HS2-B7C66H). Needs the managed standalone install available to symlink.
    #[arg(long = "shared-daemon")]
    shared_daemon: bool,
}

#[derive(Args)]
pub(crate) struct WorkArgs {
    /// The tool to drive (e.g. `claude`).
    tool: String,
    /// Project directory the tool runs in (defaults to the store path).
    #[arg(long)]
    project: Option<PathBuf>,
    /// Maximum turns before stopping (a hard safety cap).
    #[arg(long, default_value_t = 50)]
    max: u32,
    /// Stop after this many consecutive turns that change nothing (thrash guard).
    #[arg(long = "max-stall", default_value_t = 3)]
    max_stall: u32,
    /// Register connections as a self-claim worker rather than the main session.
    #[arg(long)]
    worker: bool,
    /// Codex only: drive the shared app-server **daemon** for the isolated CODEX_HOME —
    /// one codex instance reused across the whole loop's turns instead of a fresh
    /// process per turn (HS2-B7C66H); the daemon is stopped when the loop ends.
    #[arg(long = "shared-daemon")]
    shared_daemon: bool,
}

#[derive(Subcommand)]
pub(crate) enum PluginCmd {
    /// List built-in + installed plugins, with provenance and whether each is detected.
    List,
    /// Show what a plugin declares it will write + launch (the trust disclosure).
    Info { id: String },
    /// Install an external plugin from a directory into the machine plugin dir.
    Install {
        dir: PathBuf,
        /// Skip the confirmation prompt (non-interactive install).
        #[arg(long)]
        yes: bool,
    },
    /// Verify a plugin structurally (loads, known MCP format, safe write targets).
    Verify { id: String },
    /// Remove an installed external plugin by id.
    Remove { id: String },
}

#[derive(Subcommand)]
pub(crate) enum SettingsCmd {
    /// Read a setting (effective precedence global < shared < local, unless --scope is given).
    Get {
        key: String,
        /// global | shared | local (default: the effective value).
        #[arg(long)]
        scope: Option<String>,
    },
    /// Write a setting. Value is parsed as JSON if possible, else stored as a string.
    Set {
        key: String,
        value: String,
        /// global | shared | local (default: shared).
        #[arg(long)]
        scope: Option<String>,
    },
    /// List settings (effective, unless --scope is given).
    List {
        #[arg(long)]
        scope: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum AiSettingsCmd {
    /// Print the effective defaults: this project's, else the machine-wide fallback.
    Get {
        #[arg(long)]
        json: bool,
        /// Read only the machine-wide fallback.
        #[arg(long)]
        global: bool,
    },
    /// Save this project's default provider with its model and effort (machine-local project
    /// settings), or the machine-wide fallback.
    Set {
        #[arg(long)]
        tool: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
        /// The chat permission mode (HS2-NHRDMW): a Claude `--permission-mode` such as
        /// `acceptEdits`/`auto`/`plan`, or a Codex approval policy such as `on-request`.
        #[arg(long)]
        permission_mode: Option<String>,
        /// Save the machine-wide fallback used by projects without their own choice.
        #[arg(long)]
        global: bool,
    },
    /// Save one provider's default model and effort without changing the default provider
    /// (HS2-EK24KF); used whenever that provider is picked without an explicit model.
    SetProvider {
        #[arg(long)]
        tool: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
        /// The provider's chat permission mode (HS2-NHRDMW).
        #[arg(long)]
        permission_mode: Option<String>,
        /// Change the machine-wide fallback instead of this project.
        #[arg(long)]
        global: bool,
    },
}

/// The default "play" prompt: work the top Up Next ticket end to end, headless.
pub(crate) const DEFAULT_TRIGGER_PROMPT: &str = "Read the Hot Sheet Up Next queue (hotsheet tools or \
`hotsheet-cli ls --up-next`), take the highest-priority ticket, set it started, implement \
it, and mark it completed with a note on what you did. If nothing is Up Next, say so and \
stop.";

/// The CLI's per-turn event sink: stream assistant output to stdout, note permission asks.
pub(crate) fn stream_to_stdout(ev: &hotsheet_aitools::TurnEvent) {
    use hotsheet_aitools::TurnEvent;
    match ev {
        TurnEvent::Output(text) => {
            print!("{text}");
            let _ = std::io::Write::flush(&mut std::io::stdout());
        }
        TurnEvent::PermissionAsked(p) => {
            eprintln!("\n[permission] {} — {}", p.tool, p.summary)
        }
        TurnEvent::Usage(u) => {
            eprintln!("\n[usage] {} in / {} out", u.tokens_in, u.tokens_out)
        }
        // Native activity is for the attributed server sink. Do not dump raw tool inputs
        // into a CLI transcript; assistant output remains the human-facing stream.
        TurnEvent::NativeActivity { .. } => {}
        TurnEvent::Done(_) => {}
    }
}

/// The per-turn prompt for the `work` loop: like the trigger default, but explicit that
/// exactly one ticket should be taken this turn (the loop drives the next one).
pub(crate) const DEFAULT_TRIGGER_PROMPT_LOOP: &str = "Read the Hot Sheet Up Next queue (hotsheet tools \
or `hotsheet-cli ls --up-next`) and take ONLY the single highest-priority ticket: set it \
started, implement it, and mark it completed with a note on what you did. Do just that one \
ticket this turn, then stop. If nothing is Up Next, say so and stop.";

pub(crate) fn choose_launch_source(project: &Path, candidates: &[PathBuf]) -> Result<PathBuf> {
    use std::io::Write;
    if candidates.is_empty() {
        let proposed = PathBuf::from(format!("{}.hs2", project.display()));
        eprint!(
            "No ticket source found. [c]reate {}, [s]elect existing, or [q]uit: ",
            proposed.display()
        );
        std::io::stderr().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if answer.trim().eq_ignore_ascii_case("c") {
            FsStore::init(&proposed, &StoreMetadata::new("HS"))?;
            git_init(&proposed);
            return Ok(proposed.canonicalize().unwrap_or(proposed));
        }
        if answer.trim().eq_ignore_ascii_case("s") {
            eprint!("Existing ticket store path: ");
            std::io::stderr().flush()?;
            answer.clear();
            std::io::stdin().read_line(&mut answer)?;
            let path = PathBuf::from(answer.trim());
            FsStore::open(&path)
                .with_context(|| format!("{} is not a Hot Sheet ticket store", path.display()))?;
            return Ok(path.canonicalize().unwrap_or(path));
        }
        bail!("ticket-source onboarding cancelled")
    }
    eprintln!("Choose a ticket source for {}:", project.display());
    for (index, path) in candidates.iter().enumerate() {
        eprintln!("  {}) {}", index + 1, path.display());
    }
    eprint!("Selection (or 0 to cancel): ");
    std::io::stderr().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    let selection = answer.trim().parse::<usize>().unwrap_or(0);
    hotsheet_cli::select_launch_candidate(candidates, selection)
        .context("ticket-source onboarding cancelled")
}

/// POST the ask to the server's `/permissions/ask`, returning its JSON reply.
/// The running Hot Sheet server a hook should talk to: the project's discovered instance, else
/// the URL and secret the terminal was launched with.
pub(crate) fn hook_server_route() -> Option<(String, String)> {
    let project = std::env::var("HOTSHEET_PROJECT").unwrap_or_default();
    let current = (!project.is_empty())
        .then(|| {
            hotsheet_cli::external_launch::discover_running_server(
                Path::new(&project),
                &hotsheet_plugins::hotsheet_home(),
            )
            .ok()
        })
        .flatten();
    current
        .map(|server| (server.url, server.secret))
        .or_else(|| {
            Some((
                std::env::var("HOTSHEET_SERVER").ok()?,
                std::env::var("HOTSHEET_SECRET").ok()?,
            ))
        })
}

pub(crate) fn hook_get_json(url: &str, secret: &str) -> Result<serde_json::Value> {
    let body = ureq::get(url)
        .set("X-Hotsheet-Secret", secret)
        .timeout(std::time::Duration::from_secs(5))
        .call()?
        .into_string()?;
    Ok(serde_json::from_str(&body)?)
}

/// Report a session lifecycle event against the hook's terminal: `POST` (halted) or `DELETE`
/// (resumed) `/terminals/{id}/halt`, and `POST` (session started) or `DELETE` (ended)
/// `/terminals/{id}/ai-connection`.
pub(crate) fn report_terminal_session(
    url: &str,
    secret: &str,
    terminal: &str,
    session: &hotsheet_cli::permission_hook::SessionHookEvent,
    agent: Option<&str>,
    session_id: Option<&str>,
) -> Result<()> {
    use hotsheet_cli::permission_hook::SessionHookEvent;
    let endpoint = |route: &str| {
        format!(
            "{}/terminals/{}/{route}",
            url.trim_end_matches('/'),
            urlencoding_component(terminal)
        )
    };
    let post = |route: &str, body: serde_json::Value| -> Result<()> {
        ureq::post(&endpoint(route))
            .set("X-Hotsheet-Secret", secret)
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(5))
            .send_string(&body.to_string())?;
        Ok(())
    };
    let delete = |route: &str, session_id: Option<&str>| -> Result<()> {
        let url = match session_id.filter(|id| !id.is_empty()) {
            Some(id) => format!(
                "{}?session_id={}",
                endpoint(route),
                urlencoding_component(id)
            ),
            None => endpoint(route),
        };
        ureq::delete(&url)
            .set("X-Hotsheet-Secret", secret)
            .timeout(std::time::Duration::from_secs(5))
            .call()?;
        Ok(())
    };
    match session {
        SessionHookEvent::QuestionAsked {
            question,
            tool_use_id,
            questions,
        } => post(
            "question",
            serde_json::json!({ "question": question, "tool_use_id": tool_use_id, "session_id": session_id, "questions": questions }),
        ),
        SessionHookEvent::QuestionResolved { tool_use_id } => {
            let mut url = format!(
                "{}?tool_use_id={}",
                endpoint("question"),
                urlencoding_component(tool_use_id)
            );
            if let Some(session_id) = session_id {
                url.push_str("&session_id=");
                url.push_str(&urlencoding_component(session_id));
            }
            ureq::delete(&url)
                .set("X-Hotsheet-Secret", secret)
                .timeout(std::time::Duration::from_secs(5))
                .call()?;
            Ok(())
        }
        SessionHookEvent::Halted {
            error_type,
            message,
        } => post(
            "halt",
            serde_json::json!({ "error_type": error_type, "message": message, "agent": agent }),
        ),
        SessionHookEvent::Resumed => delete("halt", None),
        SessionHookEvent::Connected => post(
            "ai-connection",
            connection_body(agent, session_id, &|key| std::env::var(key).ok()),
        ),
        SessionHookEvent::Disconnected => delete("ai-connection", session_id),
    }
}

/// The `SessionStart` report body: the agent, its session id, and the address the server
/// messages the session through (`docs/13` §13.12).
/// - Codex queues messages per `CODEX_HOME`, so the server can reach the session only
///   through the home it runs under (HS2-FKM97G). Unset means the default home.
/// - Claude exports its session inbox (socket path and per-session token) to hooks; the
///   report adds both plus the Claude version (HS2-M15CHN). The token stays in server memory
///   and is never served back.
pub(crate) fn connection_body(
    agent: Option<&str>,
    session_id: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "agent": agent,
        "session_id": session_id,
        "codex_home": (agent == Some("codex"))
            .then(|| env("CODEX_HOME"))
            .flatten()
            .filter(|home| !home.trim().is_empty()),
    });
    if agent == Some("claude")
        && let Some(socket) =
            env("CLAUDE_CODE_MESSAGING_SOCKET").filter(|socket| !socket.trim().is_empty())
    {
        body["messaging_socket"] = socket.into();
        if let Some(token) = env("CLAUDE_CODE_MESSAGING_TOKEN").filter(|token| !token.is_empty()) {
            body["messaging_token"] = token.into();
        }
        if let Some(version) = hotsheet_aitools::session_push::claude_version_from_env(
            env("AI_AGENT").as_deref(),
            env("CLAUDE_CODE_EXECPATH").as_deref(),
        ) {
            body["tool_version"] = version.into();
        }
    }
    body
}

pub(crate) fn wait_for_terminal_question_answer(
    url: &str,
    secret: &str,
    terminal: &str,
    tool_use_id: &str,
    session_id: Option<&str>,
) -> Option<serde_json::Value> {
    let mut endpoint = format!(
        "{}/terminals/{}/question?tool_use_id={}",
        url.trim_end_matches('/'),
        urlencoding_component(terminal),
        urlencoding_component(tool_use_id),
    );
    if let Some(session_id) = session_id {
        endpoint.push_str("&session_id=");
        endpoint.push_str(&urlencoding_component(session_id));
    }
    // Claude's installed hook timeout is just over one day. Leave a small margin so Claude
    // gets its native prompt if nobody answers or the server disappears.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(86_400);
    while std::time::Instant::now() < deadline {
        match ureq::get(&endpoint)
            .set("X-Hotsheet-Secret", secret)
            .timeout(std::time::Duration::from_secs(5))
            .call()
        {
            Ok(response) if response.status() == 204 => {}
            Ok(response) => {
                let reply: serde_json::Value =
                    serde_json::from_str(&response.into_string().ok()?).ok()?;
                if reply["native"] == true {
                    return None;
                }
                return reply.get("answers").cloned();
            }
            Err(_) => {
                // A failed poll returns Claude to its native prompt. Clear the matching notice
                // when the server is still reachable; a newer session/tool cannot be removed.
                let _ = ureq::delete(&endpoint)
                    .set("X-Hotsheet-Secret", secret)
                    .timeout(std::time::Duration::from_secs(2))
                    .call();
                return None;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(350));
    }
    None
}

pub(crate) fn ask_server(
    url: &str,
    secret: &str,
    request: &serde_json::Value,
) -> Result<serde_json::Value> {
    let endpoint = format!("{}/permissions/ask", url.trim_end_matches('/'));
    let text = ureq::post(&endpoint)
        .set("X-Hotsheet-Secret", secret)
        .set("Content-Type", "application/json")
        .send_string(&request.to_string())?
        .into_string()?;
    Ok(serde_json::from_str(&text)?)
}

/// Validate and store AI defaults, keeping choices for providers that are not installed now.
pub(crate) fn save_ai_defaults(
    store: &Path,
    cwd: &Path,
    global: bool,
    defaults: &hotsheet_plugins::AiToolDefaults,
) -> Result<hotsheet_plugins::AiToolDefaults> {
    let tools = discovered_ai_tools();
    let (settings, scope) = ai_settings_target(store, cwd, global);
    let previous = stored_ai_defaults(&settings, scope)?;
    let prepared =
        hotsheet_plugins::prepare_ai_defaults_for_save(&tools, defaults, previous.as_ref())
            .map_err(anyhow::Error::msg)?;
    settings.set("ai.defaults", serde_json::to_value(&prepared)?, scope)?;
    Ok(hotsheet_plugins::sanitize_ai_defaults(&tools, prepared))
}

/// Render a JSON setting value for the terminal: a bare string prints unquoted;
/// everything else prints as compact JSON.
pub(crate) fn render_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
