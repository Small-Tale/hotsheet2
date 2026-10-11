//! AI tool setup, settings, plugins, permission hooks, launch, and driven turns.

use anyhow::Result;

use super::{Ctx, misrouted};
use crate::*;

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
