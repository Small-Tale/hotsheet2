//! HS2-Z8X4XP: repair tickets whose stored attachment filename HS2-SY15VS now refuses.
//!
//! Older Unix uploads could store a name such as `shots\a.png`, because the previous
//! basename helper kept backslashes. The parser now refuses the whole ticket, so it is
//! listed as corrupt. This repair renames each unsafe entry to the name the writer
//! would have stored ([`attachment_filename`]) and moves its payload into the nested
//! `attachments/<ticket>/<attachment>/<name>` layout.
//!
//! Containment: a payload is moved only when the stored name is already one ordinary
//! on-disk component (a backslash name on Unix) and the file at the nested or legacy
//! location is a regular file. A name with `/`, `..` or an absolute path never forms a
//! source path, so nothing outside the ticket's attachment directory is read or moved.
//! A ticket whose new names would collide is refused without any write. A repeated
//! run finds nothing to repair.

use super::*;

/// What happened to one attachment's payload file during a repair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairedPayload {
    /// Moved from `attachments/<ticket>/<attachment>/<old name>`.
    MovedFromNested,
    /// Moved from the legacy `attachments/<ticket>/<old name>` layout.
    MovedFromLegacy,
    /// Already at the repaired path, as after an interrupted earlier repair.
    AlreadyInPlace,
    /// No payload file was found for the stored name, or the stored name cannot be
    /// one on-disk component and so was never resolved to a path.
    NotFound,
}

/// One renamed attachment entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentRename {
    pub attachment_id: Ulid,
    pub from: String,
    pub to: String,
    pub payload: RepairedPayload,
}

/// A ticket the repair rewrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairedAttachmentTicket {
    pub path: PathBuf,
    pub id: Ulid,
    pub slug: String,
    pub renames: Vec<AttachmentRename>,
}

/// A ticket with unsafe attachment names that the repair left untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedAttachmentTicket {
    pub path: PathBuf,
    pub id: Option<Ulid>,
    pub slug: Option<String>,
    pub reason: String,
}

/// The result of [`FsStore::repair_unsafe_attachment_names`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachmentNameRepairReport {
    pub repaired: Vec<RepairedAttachmentTicket>,
    pub refused: Vec<RefusedAttachmentTicket>,
}

/// A planned rename with its resolved payload move.
struct PlannedRename {
    index: usize,
    to: String,
    source: Option<(PathBuf, RepairedPayload)>,
    target: PathBuf,
    payload: RepairedPayload,
}

impl FsStore {
    /// Rename every attachment entry whose filename the parser refuses, and its payload,
    /// to the sanitized name (HS2-Z8X4XP). Only tickets that fail to parse because of an
    /// unsafe attachment filename are touched; tickets corrupt for any other reason are
    /// left for their own recovery. Idempotent: a clean store reports nothing.
    pub fn repair_unsafe_attachment_names(
        &self,
        now: Timestamp,
    ) -> Result<AttachmentNameRepairReport, StoreError> {
        self.with_ticket_transaction(|| {
            let mut report = AttachmentNameRepairReport::default();
            let mut candidates = Vec::new();
            for path in self.ticket_file_paths()? {
                let Ok(text) = fs::read_to_string(&path) else {
                    continue;
                };
                if matches!(
                    parse_file(&text),
                    Err(ParseError::UnsafeAttachmentFilename { .. })
                ) {
                    candidates.push((path, text));
                }
            }
            if candidates.is_empty() {
                return Ok(report);
            }
            candidates.sort_by(|a, b| a.0.cmp(&b.0));
            self.ensure_current_writer_format()?;
            for (path, text) in candidates {
                match self.repair_one_ticket(&path, &text, &now)? {
                    Ok(repaired) => report.repaired.push(repaired),
                    Err(reason) => {
                        let (id, slug) = recover_ticket_identity(&path);
                        report.refused.push(RefusedAttachmentTicket {
                            path,
                            id,
                            slug,
                            reason,
                        });
                    }
                }
            }
            Ok(report)
        })
    }

    /// Repair one ticket file. The inner `Err` is a refusal reason; nothing was written.
    fn repair_one_ticket(
        &self,
        path: &Path,
        text: &str,
        now: &Timestamp,
    ) -> Result<Result<RepairedAttachmentTicket, String>, StoreError> {
        let mut ticket = match hotsheet_model::parse_file_allowing_unsafe_attachment_names(text) {
            Ok(ticket) => ticket,
            Err(error) => return Ok(Err(error.user_message())),
        };
        if self.ticket_path(&ticket.id) != path {
            return Ok(Err(format!(
                "the file is not at the path of its ticket id {}",
                ticket.id
            )));
        }
        let plan = match self.plan_renames(&ticket) {
            Ok(plan) => plan,
            Err(reason) => return Ok(Err(reason)),
        };

        let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
        let rollback = |moved: &[(PathBuf, PathBuf)]| {
            for (from, to) in moved.iter().rev() {
                // Best effort: the original failure is what the caller must see.
                let _ = fs::rename(to, from);
            }
        };
        for item in &plan {
            let Some((source, _)) = &item.source else {
                continue;
            };
            let moved_result = item
                .target
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| fs::rename(source, &item.target));
            if let Err(source_error) = moved_result {
                rollback(&moved);
                return Err(StoreError::IoAt {
                    operation: "moving attachment payload",
                    path: source.clone(),
                    source: source_error,
                });
            }
            moved.push((source.clone(), item.target.clone()));
        }

        let mut renames = Vec::with_capacity(plan.len());
        for item in plan {
            let attachment = &mut ticket.attachments[item.index];
            renames.push(AttachmentRename {
                attachment_id: attachment.id,
                from: std::mem::replace(&mut attachment.filename, item.to.clone()),
                to: item.to,
                payload: item.payload,
            });
        }
        ticket.updated_at = now.clone();
        if let Err(error) =
            self.write_ticket_with_paths_committing(&ticket, &[self.attachment_dir(&ticket.id)])
        {
            rollback(&moved);
            return Err(error);
        }
        Ok(Ok(RepairedAttachmentTicket {
            path: path.to_path_buf(),
            id: ticket.id,
            slug: ticket.slug,
            renames,
        }))
    }

    /// Plan every rename for one ticket, or the reason the ticket must be refused.
    fn plan_renames(&self, ticket: &Ticket) -> Result<Vec<PlannedRename>, String> {
        let dir = self.attachment_dir(&ticket.id);
        let mut plan = Vec::new();
        for (index, attachment) in ticket.attachments.iter().enumerate() {
            if hotsheet_model::safe_attachment_component(&attachment.filename).is_ok() {
                continue;
            }
            let to = attachment_filename(&attachment.filename);
            let nested_dir = dir.join(attachment.id.to_string());
            let target = nested_dir.join(&to);
            let source = on_disk_component(&attachment.filename).and_then(|raw| {
                [
                    (nested_dir.join(raw), RepairedPayload::MovedFromNested),
                    (dir.join(raw), RepairedPayload::MovedFromLegacy),
                ]
                .into_iter()
                .find(|(candidate, _)| is_regular_file(candidate))
            });
            let target_present = fs::symlink_metadata(&target).is_ok();
            if source.is_some() && target_present {
                return Err(format!(
                    "attachment {} would be renamed to '{to}', but a file with that name \
                     already exists in its attachment directory",
                    attachment.id
                ));
            }
            // A legacy-layout file with the new name would be adopted as a second
            // attachment on the next read.
            let legacy_twin = dir.join(&to);
            if fs::symlink_metadata(&legacy_twin).is_ok()
                && source.as_ref().is_none_or(|(path, _)| path != &legacy_twin)
            {
                return Err(format!(
                    "attachment {} would be renamed to '{to}', but the ticket's attachment \
                     directory already holds a file with that name",
                    attachment.id
                ));
            }
            let payload = match &source {
                Some((_, kind)) => *kind,
                None if target_present => RepairedPayload::AlreadyInPlace,
                None => RepairedPayload::NotFound,
            };
            plan.push(PlannedRename {
                index,
                to,
                source,
                target,
                payload,
            });
        }

        let mut seen = std::collections::HashMap::<String, Ulid>::new();
        for (index, attachment) in ticket.attachments.iter().enumerate() {
            let name = plan
                .iter()
                .find(|item| item.index == index)
                .map_or(attachment.filename.as_str(), |item| item.to.as_str());
            if let Some(other) = seen.insert(name.to_lowercase(), attachment.id) {
                return Err(format!(
                    "attachments {other} and {} would both be named '{name}'; rename one \
                     of them by hand",
                    attachment.id
                ));
            }
        }
        Ok(plan)
    }
}

/// The stored name as one ordinary on-disk component, or `None` when it could not
/// name a file directly inside a directory on this platform (separators that the
/// platform honours, `.`/`..`, absolute paths, NUL, empty).
fn on_disk_component(name: &str) -> Option<&str> {
    if name.is_empty() || name.contains(['/', '\0']) || name == "." || name == ".." {
        return None;
    }
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(component)), None) if component == name => Some(name),
        _ => None,
    }
}

/// A regular file, never a symlink or directory, so a move cannot follow a link out.
fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

#[cfg(test)]
#[path = "store_attachment_name_repair_tests.rs"]
mod tests;
