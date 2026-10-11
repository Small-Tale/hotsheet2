//! Git-backed ticket, note, attachment, transfer, and people commands.

use anyhow::Result;

use super::{Ctx, misrouted};
use crate::*;

pub(super) fn run(cmd: Cmd, ctx: &Ctx) -> Result<()> {
    let path = &ctx.path;
    let actor = ctx.actor.as_ref();
    match cmd {
        Cmd::New(args) => cmd_new(path, args),
        Cmd::Ls { filters } => cmd_ls(path, &filters),
        Cmd::Show { id } => cmd_show(path, &id),
        Cmd::Batch { ids, update } => cmd_batch(actor, path, &ids, update),
        Cmd::DeleteNote { id, note_id } => cmd_delete_note(path, &id, &note_id),
        Cmd::Attach(args) => cmd_attach(path, args),
        Cmd::AttachmentRename {
            id,
            attachment,
            filename,
        } => cmd_attachment_rename(path, &id, &attachment, &filename),
        Cmd::AttachmentDelete { id, attachment } => cmd_attachment_delete(path, &id, &attachment),
        Cmd::AttachmentMetadata {
            id,
            attachments,
            file,
        } => cmd_attachment_metadata(path, &id, &attachments, &file),
        Cmd::Annotate {
            id,
            attachment,
            file,
            clear,
            json,
        } => cmd_annotate(path, &id, &attachment, file.as_deref(), clear, json, actor),
        Cmd::AttachmentActor {
            id,
            attachment_ids,
            actor_role,
            actor_id,
            actor_name,
        } => cmd_attachment_actor(
            path,
            &id,
            &attachment_ids,
            &actor_role,
            actor_id,
            actor_name,
        ),
        Cmd::Edit { id, update } => cmd_edit(actor, path, &id, TicketEdit::prepare(update)?, false),
        Cmd::Close {
            id,
            reason,
            duplicate_of,
        } => cmd_close(actor, path, &id, &reason, duplicate_of),
        Cmd::Restore { id } => cmd_restore(path, &id),
        Cmd::Copy { id, to } => cmd_copy(path, &id, &to),
        Cmd::Move { id, to, yes } => cmd_move(path, &id, &to, yes),
        Cmd::Assign {
            id,
            to,
            clear,
            review,
        } => cmd_assign(path, &id, to, clear, review),
        Cmd::People { cmd } => cmd_people(path, cmd),
        Cmd::Read { id } => cmd_read(path, &id),
        Cmd::ConfidenceReport { json } => cmd_confidence_report(path, json),
        Cmd::Metrics {
            roll_up,
            prune_before,
            team,
        } => cmd_metrics(path, roll_up, prune_before, team),
        _ => misrouted("tickets"),
    }
}
