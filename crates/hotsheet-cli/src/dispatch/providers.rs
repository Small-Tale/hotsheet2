//! External ticket-provider connections, provider-native tickets, keys, and accounts.

use anyhow::{Result, bail};
use hotsheet_ticketing::FsStore;

use super::{Ctx, misrouted};
use crate::*;

pub(super) fn run(cmd: Cmd, ctx: &Ctx) -> Result<()> {
    let path = &ctx.path;
    let actor = ctx.actor.as_ref();
    match cmd {
        Cmd::Providers { json } => cmd_providers(path, json),
        Cmd::GithubSignIn { web_base } => cmd_github_sign_in(&web_base),
        Cmd::GithubConnect(args) => cmd_github_connect(path, args),
        Cmd::ProviderAttach {
            connection,
            id,
            files,
        } => cmd_provider_attach(path, &connection, &id, &files),
        Cmd::ProviderLs { connection } => cmd_provider_ls(path, &connection),
        Cmd::ProviderGet { connection, id } => cmd_provider_get(path, &connection, &id),
        Cmd::FeedbackSynthesis { command } => match command {
            FeedbackSynthesisCmd::Prepare { connections } => {
                cmd_feedback_synthesis_prepare(path, &connections)
            }
            FeedbackSynthesisCmd::Accept { reviewed } => {
                accept_feedback_synthesis(path, actor, reviewed)
            }
        },
        Cmd::ProviderDisable { connection } => cmd_provider_set_disabled(path, &connection, true),
        Cmd::ProviderEnable { connection } => cmd_provider_set_disabled(path, &connection, false),
        Cmd::ProviderRemove { connection, json } => cmd_provider_remove(path, &connection, json),
        Cmd::ProviderNew {
            connection,
            title,
            category,
            priority,
            details,
            tags,
        } => cmd_provider_new(
            path,
            &connection,
            title,
            category,
            &priority,
            details.unwrap_or_default(),
            tags,
        ),
        Cmd::ProviderEdit {
            connection,
            id,
            title,
            details,
            status,
            expected_token,
            note,
            note_file,
            allow_literal_backslash_n,
            note_kind,
            note_summary,
            note_confidence,
            clear_note_confidence,
            edit_note,
        } => {
            let note = read_note_input(note, note_file, allow_literal_backslash_n)?;
            let note_confidence = confidence_change(note_confidence, clear_note_confidence);
            validate_note_modifiers(
                &note,
                note_kind.as_ref(),
                note_summary.as_ref(),
                note_confidence,
                edit_note.as_ref(),
            )?;
            cmd_provider_edit(
                actor,
                path,
                &connection,
                &id,
                ProviderEditInput {
                    title,
                    details,
                    status,
                    expected_token,
                    note,
                    note_kind: parse_note_kind(note_kind.as_deref().unwrap_or("regular"))?,
                    note_summary,
                    note_confidence,
                    edit_note,
                },
            )
        }
        Cmd::ProviderClose {
            connection,
            id,
            reason,
        } => cmd_provider_close(path, &connection, &id, &reason),
        Cmd::ProviderAssign {
            connection,
            id,
            to,
            clear,
            reviews,
        } => cmd_provider_assign(path, &connection, &id, to, clear, reviews),
        Cmd::ProviderRestore { connection, id } => cmd_provider_restore(path, &connection, &id),
        Cmd::ProviderReportNotWorking {
            connection,
            id,
            note,
            note_file,
            evidence,
            expected_token,
        } => cmd_provider_report_not_working(
            path,
            &connection,
            &id,
            note,
            note_file,
            &evidence,
            expected_token,
        ),
        Cmd::ProviderDeleteNote {
            connection,
            id,
            note_id,
        } => cmd_provider_delete_note(path, &connection, &id, &note_id),
        Cmd::ProviderCopy {
            id,
            to,
            operation_id,
        } => cmd_provider_transfer(path, &id, &to, &operation_id, false),
        Cmd::ProviderMove {
            id,
            to,
            operation_id,
            yes,
        } => {
            if !yes {
                bail!("provider-move requires --yes");
            }
            cmd_provider_transfer(path, &id, &to, &operation_id, true)
        }
        Cmd::Key { cmd } => cmd_key(cmd),
        Cmd::Account { cmd } => cmd_account(cmd, path),
        _ => misrouted("providers"),
    }
}

fn accept_feedback_synthesis(
    path: &Path,
    actor: Option<&hotsheet_ticketing::actor::MutationActor>,
    reviewed: bool,
) -> Result<()> {
    if !reviewed {
        bail!("review the draft, then pass --reviewed to accept its source cursor");
    }
    if actor.map(|actor| actor.role) != Some(hotsheet_model::AttachmentActorRole::Human) {
        bail!("accepting AI feedback synthesis requires --actor-role human");
    }
    let dir = feedback_synthesis::state_dir(FsStore::open(path)?.root())?;
    let cursor = feedback_synthesis::accept_review(&dir)?;
    println!("Marked feedback reviewed through {cursor}. Repository guidance was not published.");
    Ok(())
}
