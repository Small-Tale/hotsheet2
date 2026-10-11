//! The local server: serving, certificates, lifecycle, and live server-backed reads.

use anyhow::Result;

use super::{Ctx, misrouted};
use crate::*;

pub(super) fn run(cmd: Cmd, ctx: &Ctx) -> Result<()> {
    let path = &ctx.path;
    match cmd {
        Cmd::Commands { cmd } => cmd_commands(path, cmd),
        Cmd::Lifecycle { cmd } => match cmd {
            LifecycleCmd::Status => print_local_server_json(path, "GET", "lifecycle/quiescence"),
            LifecycleCmd::Restart => print_local_server_json(path, "POST", "lifecycle/restart"),
        },
        Cmd::TicketFlow => print_local_server_json(path, "GET", "analytics/tickets"),
        Cmd::Activity {
            ticket,
            session,
            min_importance,
            limit,
        } => cmd_activity(path, ticket, session, min_importance, limit),
        Cmd::Notifications { cmd } => cmd_notifications(path, cmd),
        Cmd::Serve {
            bind,
            secret,
            stop,
            kill_all_terminals,
            list,
        } => cmd_serve(path, &bind, secret, stop, kill_all_terminals, list),
        Cmd::Cert { cmd } => cmd_cert(path, &cmd),
        _ => misrouted("server"),
    }
}
