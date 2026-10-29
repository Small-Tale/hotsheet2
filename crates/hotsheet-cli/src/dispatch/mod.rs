//! `hotsheet-cli` command dispatch, split by command group (HS2-X4N5RH).
//!
//! `main` parses [`Cmd`], resolves the store, and hands the command to [`run`]. Every
//! variant is assigned to exactly one group by the exhaustive [`Cmd::group`] match, so a
//! new subcommand fails to compile until it is routed; each group module then matches
//! only its own variants.

mod claim_process;
pub(crate) mod claims;
pub(crate) mod coordination;
pub(crate) mod notify;
pub(crate) mod providers;
pub(crate) mod server;
pub(crate) mod setup;
pub(crate) mod store;
pub(crate) mod tickets;

use std::path::PathBuf;

use anyhow::Result;
use hotsheet_ticketing::actor::MutationActor;

use crate::Cmd;

/// Invocation-wide inputs shared by every command handler.
pub(crate) struct Ctx {
    /// The resolved store path (`-C`, `$HOTSHEET_STORE`, or a walked-up link).
    pub(crate) path: PathBuf,
    /// The process working directory.
    pub(crate) cwd: PathBuf,
    /// The acting role from `--actor-role` / `HOTSHEET_ACTOR_ROLE`, if any.
    pub(crate) actor: Option<MutationActor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Group {
    Store,
    Tickets,
    Providers,
    Claims,
    Server,
    Setup,
    Coordination,
    Notify,
}

impl Cmd {
    pub(crate) fn group(&self) -> Group {
        use Cmd::*;
        match self {
            Coordination(_) => Group::Coordination,
            Notify { .. } => Group::Notify,
            Init { .. }
            | Bootstrap { .. }
            | Link { .. }
            | ActivateFormat { .. }
            | Compatibility { .. }
            | Import { .. }
            | Checkout { .. }
            | Sync
            | Doctor { .. }
            | Reindex { .. }
            | Worklist
            | PurgeTrash { .. }
            | MergeDriver { .. } => Group::Store,
            New(_)
            | LinkFollowUp { .. }
            | Ls { .. }
            | Show { .. }
            | Batch { .. }
            | DeleteNote { .. }
            | Attach(_)
            | AttachmentRename { .. }
            | AttachmentDelete { .. }
            | AttachmentMetadata { .. }
            | Annotate { .. }
            | AttachmentActor { .. }
            | Edit { .. }
            | Close { .. }
            | Restore { .. }
            | Copy { .. }
            | Move { .. }
            | Assign { .. }
            | People { .. }
            | Read { .. }
            | ConfidenceReport { .. }
            | PhaseReport { .. }
            | Metrics { .. }
            | Reprice { .. } => Group::Tickets,
            Providers { .. }
            | GithubSignIn { .. }
            | GithubConnect(_)
            | GithubWebhookSetup { .. }
            | ProviderConnect(_)
            | ProviderAttach { .. }
            | ProviderLs { .. }
            | ProviderGet { .. }
            | FeedbackSynthesis { .. }
            | ProviderDisable { .. }
            | ProviderEnable { .. }
            | ProviderRemove { .. }
            | ProviderNew { .. }
            | ProviderEdit { .. }
            | ProviderClose { .. }
            | ProviderAssign { .. }
            | ProviderRestore { .. }
            | ProviderReportNotWorking { .. }
            | ProviderDeleteNote { .. }
            | ProviderCopy { .. }
            | ProviderMove { .. }
            | Key { .. }
            | Account { .. } => Group::Providers,
            ClaimNext { .. } | Claim { .. } | Release { .. } | Renew { .. } | ClaimRun { .. } => {
                Group::Claims
            }
            #[cfg(windows)]
            ClaimRunChild { .. } => Group::Claims,
            Commands { .. }
            | Terminal { .. }
            | Lifecycle { .. }
            | TicketFlow
            | Activity { .. }
            | Notifications { .. }
            | Processes
            | Serve { .. }
            | Cert { .. } => Group::Server,
            Setup { .. }
            | Guidance { .. }
            | Plugin { .. }
            | AiTools { .. }
            | AiSettings { .. }
            | Settings { .. }
            | PermissionHook { .. }
            | PermissionDiscard { .. }
            | HookDiagnose { .. }
            | Launch { .. }
            | Trigger(_)
            | Work(_) => Group::Setup,
        }
    }

    /// Whether `-C` is resolved through `$HOTSHEET_STORE` / a `.hotsheet2/store` link
    /// (HS2-5CXKZ0). `init`/`link` and friends operate on the literal path instead.
    pub(crate) fn resolves_store_path(&self) -> bool {
        #[cfg(windows)]
        if matches!(self, Cmd::ClaimRunChild { .. }) {
            return false;
        }
        !matches!(
            self,
            Cmd::Init { .. }
                | Cmd::Bootstrap { .. }
                | Cmd::Link { .. }
                | Cmd::Checkout { .. }
                | Cmd::Launch { .. }
                | Cmd::HookDiagnose { .. }
                | Cmd::PermissionDiscard { .. }
                | Cmd::Processes
                | Cmd::Guidance { .. }
                | Cmd::Notify { .. }
                | Cmd::Serve { list: true, .. }
        )
    }

    /// Whether a successful run refreshes the registered checkouts' worklists.
    pub(crate) fn refreshes_worklists(&self) -> bool {
        #[cfg(windows)]
        if matches!(self, Cmd::ClaimRunChild { .. }) {
            return false;
        }
        !matches!(
            self,
            Cmd::ClaimRun { .. }
                | Cmd::Init { .. }
                | Cmd::Coordination(_)
                | Cmd::Bootstrap { .. }
                | Cmd::Link { .. }
                | Cmd::Checkout { .. }
                | Cmd::Commands { .. }
                | Cmd::Terminal { .. }
                | Cmd::Lifecycle { .. }
                | Cmd::TicketFlow
                | Cmd::Activity { .. }
                | Cmd::Notifications { .. }
                | Cmd::Notify { .. }
                | Cmd::Launch { .. }
                | Cmd::HookDiagnose { .. }
                | Cmd::PermissionDiscard { .. }
                | Cmd::Processes
                | Cmd::Guidance { .. }
                | Cmd::Setup { diagnose: true, .. }
                | Cmd::Serve { .. }
                | Cmd::Ls { .. }
                | Cmd::Show { .. }
                | Cmd::Reindex { .. }
                | Cmd::Import {
                    diagnose_attachments: true,
                    ..
                }
        )
    }
}

/// Run one parsed command through its group's handler.
pub(crate) fn run(cmd: Cmd, ctx: &Ctx) -> Result<()> {
    match cmd.group() {
        Group::Store => store::run(cmd, ctx),
        Group::Tickets => tickets::run(cmd, ctx),
        Group::Providers => providers::run(cmd, ctx),
        Group::Claims => claims::run(cmd, ctx),
        Group::Server => server::run(cmd, ctx),
        Group::Setup => setup::run(cmd, ctx),
        Group::Coordination => coordination::run(cmd, ctx),
        Group::Notify => notify::run(cmd, ctx),
    }
}

/// A group handler received a command [`Cmd::group`] routes elsewhere.
fn misrouted(group: &str) -> ! {
    unreachable!("command routed to the {group} dispatch group by mistake")
}
