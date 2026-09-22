//! Explicit repair of attachment copies made before resumable HS1 import checkpoints.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use hotsheet_model::Ulid;
use hotsheet_ticketing::{FsStore, StoreError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::import::{
    ExportAttachment, ExportFile, SUPPORTED_EXPORT_VERSION, attachment_id, import_id,
};

pub const OMISSIONS_FILE: &str = "hotsheet-hs1-attachment-omissions.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentState {
    Present,
    MissingTicket,
    MissingMetadata,
    MissingPayload,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentDiagnostic {
    pub selection: String,
    pub ticket_id: Ulid,
    pub ticket_slug: Option<String>,
    pub attachment_id: Ulid,
    pub source_filename: String,
    pub destination_filename: Option<String>,
    pub state: AttachmentState,
    pub pending_import: bool,
    pub source_sha256: Option<String>,
    pub source_error: Option<String>,
    pub omission_confirmed: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfirmedOmission {
    ticket_id: Ulid,
    attachment_id: Ulid,
    source_filename: String,
    source_sha256: String,
    export_fingerprint: String,
    confirmed_at_unix: i64,
}

#[derive(Deserialize, Serialize)]
struct Omissions {
    version: u32,
    attachments: BTreeMap<String, ConfirmedOmission>,
}

impl Omissions {
    fn read(store: &FsStore) -> Result<Self> {
        let path = store.root().join(OMISSIONS_FILE);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    version: 1,
                    attachments: BTreeMap::new(),
                });
            }
            Err(error) => return Err(error.into()),
        };
        let value: Self =
            serde_json::from_slice(&bytes).context("reading HS1 attachment omission evidence")?;
        if value.version != 1 {
            bail!(
                "unsupported HS1 attachment omission evidence version {}",
                value.version
            );
        }
        Ok(value)
    }

    fn write(&self, store: &FsStore) -> Result<()> {
        let mut staged = tempfile::NamedTempFile::new_in(store.root())?;
        serde_json::to_writer_pretty(&mut staged, self)?;
        staged.write_all(b"\n")?;
        staged.as_file().sync_all()?;
        staged.persist(store.root().join(OMISSIONS_FILE))?;
        Ok(())
    }
}

fn export(path: &Path) -> Result<ExportFile> {
    let value: ExportFile = serde_json::from_slice(&std::fs::read(path)?)?;
    if value.export_version != SUPPORTED_EXPORT_VERSION {
        bail!("attachment recovery requires exportVersion {SUPPORTED_EXPORT_VERSION}");
    }
    Ok(value)
}

fn source_path(base: &Path, attachment: &ExportAttachment) -> Result<PathBuf> {
    if Path::new(&attachment.stored_path).is_absolute() {
        bail!("recovery requires a portable export with relative attachment paths");
    }
    let base = base.canonicalize()?;
    let source = base.join(&attachment.stored_path).canonicalize()?;
    if !source.starts_with(&base) {
        bail!("export attachment escapes the portable bundle");
    }
    Ok(source)
}

fn source_hash(base: &Path, attachment: &ExportAttachment) -> Result<String> {
    let mut source = std::fs::File::open(source_path(base, attachment)?)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn fingerprint(
    export: &ExportFile,
    ticket: &Ulid,
    attachment: &ExportAttachment,
    payload: &str,
) -> Result<String> {
    let identity = serde_json::json!({
        "version": export.export_version,
        "project": {"name": export.project.name, "ticketPrefix": export.project.ticket_prefix, "sourceRoot": export.project.source_root},
        "ticket": ticket,
        "attachment": attachment,
        "payloadSha256": payload,
    });
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&identity)?)
    ))
}

fn selection(ticket: &Ulid, attachment: &Ulid) -> String {
    format!("{ticket}/{attachment}")
}

fn approval_matches(
    export: &ExportFile,
    id: &Ulid,
    attachment: &ExportAttachment,
    hash: &str,
    approval: &ConfirmedOmission,
) -> Result<bool> {
    Ok(approval.ticket_id == *id
        && approval.attachment_id == attachment_id(id, attachment)
        && approval.source_sha256 == hash
        && approval.source_filename
            == attachment
                .original_filename
                .as_deref()
                .unwrap_or(&attachment.stored_path)
        && approval.export_fingerprint == fingerprint(export, id, attachment, hash)?)
}

/// Compare a portable export to an existing store without changing files, Git, or proofs.
pub fn diagnose(store_path: &Path, export_path: &Path) -> Result<Vec<AttachmentDiagnostic>> {
    let store = FsStore::open_without_maintenance(store_path)?;
    let export = export(export_path)?;
    let base = export_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let omissions = Omissions::read(&store)?;
    let mut result = Vec::new();
    let mut identities = BTreeSet::new();
    for (index, source) in export.tickets.iter().enumerate() {
        let id = import_id(&export.project, source, index);
        let ticket = match store.read_ticket(&id) {
            Ok(ticket) => Some(ticket),
            Err(StoreError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(StoreError::IoAt { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                None
            }
            Err(error) => return Err(error.into()),
        };
        for attachment in &source.attachments {
            let attachment_id = attachment_id(&id, attachment);
            let key = selection(&id, &attachment_id);
            if !identities.insert(key.clone()) {
                bail!("duplicate exported attachment identity {key}");
            }
            let metadata = ticket.as_ref().and_then(|ticket| {
                ticket
                    .attachments
                    .iter()
                    .find(|item| item.id == attachment_id)
            });
            let state = if ticket.is_none() {
                AttachmentState::MissingTicket
            } else if metadata.is_none() {
                AttachmentState::MissingMetadata
            } else {
                match store.read_attachment(&id, &attachment_id) {
                    Ok(_) => AttachmentState::Present,
                    Err(StoreError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                        AttachmentState::MissingPayload
                    }
                    Err(error) => return Err(error.into()),
                }
            };
            let (source_sha256, source_error) = match source_hash(base, attachment) {
                Ok(hash) => (Some(hash), None),
                Err(error) => (None, Some(error.to_string())),
            };
            let omission_confirmed = if state == AttachmentState::MissingMetadata {
                if let (Some(hash), Some(approval)) =
                    (&source_sha256, omissions.attachments.get(&key))
                {
                    approval_matches(&export, &id, attachment, hash, approval)?
                } else {
                    false
                }
            } else {
                false
            };
            result.push(AttachmentDiagnostic {
                selection: key,
                ticket_id: id,
                ticket_slug: ticket.as_ref().map(|ticket| ticket.slug.clone()),
                attachment_id,
                source_filename: attachment
                    .original_filename
                    .clone()
                    .unwrap_or_else(|| attachment.stored_path.clone()),
                destination_filename: metadata.map(|metadata| metadata.filename.clone()),
                state,
                pending_import: store
                    .root()
                    .join("hotsheet-hs1-import-pending")
                    .join(format!("{id}.json"))
                    .exists(),
                source_sha256,
                source_error,
                omission_confirmed,
            });
        }
    }
    Ok(result)
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverySummary {
    pub restored: usize,
    pub omissions_confirmed: usize,
    pub unchanged: usize,
}

/// Apply only explicit selections, after validating the entire request. Repeats are no-ops.
pub fn recover(
    store_path: &Path,
    export_path: &Path,
    restore: &[String],
    omit: &[String],
) -> Result<RecoverySummary> {
    let rows = diagnose(store_path, export_path)?;
    let restore: BTreeSet<_> = restore.iter().cloned().collect();
    let omit: BTreeSet<_> = omit.iter().cloned().collect();
    if restore.is_empty() && omit.is_empty() {
        bail!("select at least one attachment to restore or confirm omitted");
    }
    if let Some(key) = restore.intersection(&omit).next() {
        bail!("cannot restore and omit the same attachment: {key}");
    }
    for key in restore.union(&omit) {
        let row = rows
            .iter()
            .find(|row| &row.selection == key)
            .with_context(|| format!("attachment selection is not in this export: {key}"))?;
        if row.state == AttachmentState::MissingTicket {
            bail!(
                "ticket {} is missing; import the ticket before attachment recovery",
                row.ticket_id
            );
        }
        if row.pending_import {
            bail!(
                "ticket {} has a pending import checkpoint; retry its ordinary import before legacy recovery",
                row.ticket_id
            );
        }
        if omit.contains(key) && row.state != AttachmentState::MissingMetadata {
            bail!(
                "{key} still has destination attachment metadata; restore its payload or explicitly delete the attachment before confirming omission"
            );
        }
        if row.source_sha256.is_none() {
            bail!("{key} has no readable staged source; select a complete portable export");
        }
    }
    let store = FsStore::open_without_maintenance(store_path)?.with_deferred_push();
    let export = export(export_path)?;
    let base = export_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut omissions = Omissions::read(&store)?;
    let mut summary = RecoverySummary::default();
    let changes = rows.iter().any(|row| {
        (restore.contains(&row.selection)
            && (row.state != AttachmentState::Present
                || omissions.attachments.contains_key(&row.selection)))
            || (omit.contains(&row.selection) && !row.omission_confirmed)
    });
    if !changes {
        summary.unchanged = restore.len() + omit.len();
        return Ok(summary);
    }
    crate::import_completion::invalidate(store_path)?;
    let mut changed_paths = Vec::new();
    let mut omissions_changed = false;
    for (index, source) in export.tickets.iter().enumerate() {
        let id = import_id(&export.project, source, index);
        for attachment in &source.attachments {
            let aid = attachment_id(&id, attachment);
            let key = selection(&id, &aid);
            if restore.contains(&key) {
                // Recheck the selected source immediately before mutation.
                let hash = source_hash(base, attachment)?;
                let before = rows.iter().find(|row| row.selection == key).unwrap();
                if before.source_sha256.as_ref() != Some(&hash) {
                    bail!("selected source changed during recovery: {key}");
                }
                if crate::import::restore_missing_attachment(&store, base, &id, attachment)? {
                    summary.restored += 1;
                    changed_paths.extend([store.ticket_path(&id), store.attachment_dir(&id)]);
                } else {
                    summary.unchanged += 1;
                }
                omissions_changed |= omissions.attachments.remove(&key).is_some();
            } else if omit.contains(&key) {
                let row = rows.iter().find(|row| row.selection == key).unwrap();
                if row.omission_confirmed {
                    summary.unchanged += 1;
                    continue;
                }
                let ticket = store.read_ticket(&id)?;
                if ticket.attachments.iter().any(|item| item.id == aid) {
                    bail!("attachment reappeared during omission confirmation: {key}");
                }
                let hash = source_hash(base, attachment)?;
                if row.source_sha256.as_ref() != Some(&hash) {
                    bail!("selected source changed during confirmation: {key}");
                }
                omissions.attachments.insert(
                    key,
                    ConfirmedOmission {
                        ticket_id: id,
                        attachment_id: aid,
                        source_filename: row.source_filename.clone(),
                        export_fingerprint: fingerprint(&export, &id, attachment, &hash)?,
                        source_sha256: hash,
                        confirmed_at_unix: time::OffsetDateTime::now_utc().unix_timestamp(),
                    },
                );
                omissions_changed = true;
                summary.omissions_confirmed += 1;
            }
        }
    }
    if omissions_changed {
        omissions.write(&store)?;
        changed_paths.push(store.root().join(OMISSIONS_FILE));
    }
    if !changed_paths.is_empty() {
        store.autocommit_paths(
            "Recover explicitly selected HS1 attachments and record omissions",
            &changed_paths,
        )?;
    }
    Ok(summary)
}

/// An absent identity is acceptable only with matching, explicit source-bound evidence.
pub(crate) fn omission_approved(
    store: &FsStore,
    export: &ExportFile,
    id: &Ulid,
    attachment: &ExportAttachment,
    base: &Path,
) -> Result<bool> {
    let aid = attachment_id(id, attachment);
    if store
        .read_ticket(id)?
        .attachments
        .iter()
        .any(|item| item.id == aid)
    {
        return Ok(false);
    }
    let omissions = Omissions::read(store)?;
    let Some(approval) = omissions.attachments.get(&selection(id, &aid)) else {
        return Ok(false);
    };
    approval_matches(
        export,
        id,
        attachment,
        &source_hash(base, attachment)?,
        approval,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_ticketing::StoreMetadata;

    struct Fixture {
        directory: tempfile::TempDir,
        store: FsStore,
        file: PathBuf,
        export: ExportFile,
        id: Ulid,
    }
    fn fixture() -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        let store =
            FsStore::init(directory.path().join("store"), &StoreMetadata::new("HS2")).unwrap();
        let file = directory.path().join("export.json");
        let value = serde_json::json!({"exportVersion":1,"project":{"name":"Recovery","ticketPrefix":"HS2"},"tickets":[{
            "ticket_number":"HS-1","title":"Original title","details":"Original details","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-02T00:00:00Z",
            "attachments":[{"original_filename":"first.png","stored_path":"first.png"},{"original_filename":"second.png","stored_path":"second.png"}]
        }]});
        std::fs::write(&file, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        std::fs::write(directory.path().join("first.png"), b"FIRST").unwrap();
        std::fs::write(directory.path().join("second.png"), b"SECOND").unwrap();
        let export = export(&file).unwrap();
        crate::import::import(&store, &export, directory.path()).unwrap();
        let id = import_id(&export.project, &export.tickets[0], 0);
        Fixture {
            directory,
            store,
            file,
            export,
            id,
        }
    }
    fn remove(f: &Fixture, index: usize) -> String {
        let aid = attachment_id(&f.id, &f.export.tickets[0].attachments[index]);
        f.store
            .remove_attachment(&f.id, &aid, "2026-02-01T00:00:00Z".into())
            .unwrap();
        selection(&f.id, &aid)
    }
    fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    visit(root, &path, files);
                } else {
                    files.insert(
                        path.strip_prefix(root).unwrap().into(),
                        std::fs::read(path).unwrap(),
                    );
                }
            }
        }
        let mut files = BTreeMap::new();
        visit(root, root, &mut files);
        files
    }

    #[test]
    fn diagnostic_is_read_only_even_without_managed_gitignore_and_classifies_missing_ticket() {
        let f = fixture();
        std::fs::remove_file(f.store.root().join(".gitignore")).unwrap();
        let before = snapshot(f.directory.path());
        let rows = diagnose(f.store.root(), &f.file).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.state == AttachmentState::Present));
        assert_eq!(snapshot(f.directory.path()), before);
        std::fs::remove_file(f.store.ticket_path(&f.id)).unwrap();
        let rows = diagnose(f.store.root(), &f.file).unwrap();
        assert!(
            rows.iter()
                .all(|row| row.state == AttachmentState::MissingTicket)
        );
        assert!(recover(f.store.root(), &f.file, &[rows[0].selection.clone()], &[]).is_err());
    }

    #[test]
    fn restores_only_selected_legacy_missing_metadata_preserving_fields_and_repeat_is_noop() {
        let f = fixture();
        let first = remove(&f, 0);
        let second = remove(&f, 1);
        let mut ticket = f.store.read_ticket(&f.id).unwrap();
        ticket.title = "Edited title".into();
        ticket.details = "Edited details".into();
        ticket.tags.push("keep".into());
        f.store.write_ticket(&ticket).unwrap();
        assert!(
            !f.store
                .root()
                .join("hotsheet-hs1-import-pending")
                .join(format!("{}.json", f.id))
                .exists()
        );
        // Ordinary repeat import must not guess which absent identities were deliberate deletions.
        crate::import::import(&f.store, &f.export, f.directory.path()).unwrap();
        assert_eq!(f.store.read_ticket(&f.id).unwrap(), ticket);
        let result = recover(f.store.root(), &f.file, std::slice::from_ref(&first), &[]).unwrap();
        assert_eq!(result.restored, 1);
        let after = f.store.read_ticket(&f.id).unwrap();
        assert_eq!(
            (&after.title, &after.details, &after.tags, &after.updated_at),
            (
                &ticket.title,
                &ticket.details,
                &ticket.tags,
                &ticket.updated_at
            )
        );
        assert_eq!(after.attachments.len(), 1);
        assert_eq!(
            diagnose(f.store.root(), &f.file)
                .unwrap()
                .iter()
                .find(|row| row.selection == second)
                .unwrap()
                .state,
            AttachmentState::MissingMetadata
        );
        let before = snapshot(f.directory.path());
        assert_eq!(
            recover(f.store.root(), &f.file, &[first], &[])
                .unwrap()
                .unchanged,
            1
        );
        assert_eq!(snapshot(f.directory.path()), before);
    }

    #[test]
    fn missing_payload_repair_preserves_renames_annotations_and_provenance() {
        let f = fixture();
        let aid = attachment_id(&f.id, &f.export.tickets[0].attachments[0]);
        let mut ticket = f
            .store
            .rename_attachment(&f.id, &aid, "2026-03-01T00:00:00Z".into(), "renamed.png")
            .unwrap();
        let attachment = ticket
            .attachments
            .iter_mut()
            .find(|item| item.id == aid)
            .unwrap();
        attachment.batch_label = Some("Edited batch".into());
        attachment
            .annotations
            .push(hotsheet_model::MediaAnnotation {
                id: "note".into(),
                x: 10,
                y: 20,
                width: 30,
                height: 40,
                start_ms: None,
                end_ms: None,
                text: "Keep annotation".into(),
            });
        f.store.write_ticket(&ticket).unwrap();
        std::fs::remove_file(
            f.store
                .attachment_dir(&f.id)
                .join(aid.to_string())
                .join("renamed.png"),
        )
        .unwrap();
        let rows = diagnose(f.store.root(), &f.file).unwrap();
        assert_eq!(rows[0].state, AttachmentState::MissingPayload);
        assert_eq!(rows[0].destination_filename.as_deref(), Some("renamed.png"));
        assert!(recover(f.store.root(), &f.file, &[], &[rows[0].selection.clone()]).is_err());
        assert_eq!(
            recover(f.store.root(), &f.file, &[rows[0].selection.clone()], &[])
                .unwrap()
                .restored,
            1
        );
        assert_eq!(f.store.read_ticket(&f.id).unwrap(), ticket);
        assert_eq!(f.store.read_attachment(&f.id, &aid).unwrap().1, b"FIRST");
    }

    #[test]
    fn explicit_omission_survives_reimport_and_requires_matching_source_evidence() {
        let f = fixture();
        let key = remove(&f, 0);
        assert!(crate::import_completion::verify_export(f.store.root(), &f.file).is_err());
        let before = f.store.read_ticket(&f.id).unwrap();
        assert_eq!(
            recover(f.store.root(), &f.file, &[], std::slice::from_ref(&key))
                .unwrap()
                .omissions_confirmed,
            1
        );
        assert_eq!(f.store.read_ticket(&f.id).unwrap(), before);
        assert!(diagnose(f.store.root(), &f.file).unwrap()[0].omission_confirmed);
        crate::import::import(&f.store, &f.export, f.directory.path()).unwrap();
        crate::import_completion::verify_export(f.store.root(), &f.file).unwrap();
        let bytes = std::fs::read(f.store.root().join(OMISSIONS_FILE)).unwrap();
        assert_eq!(
            recover(f.store.root(), &f.file, &[], std::slice::from_ref(&key))
                .unwrap()
                .unchanged,
            1
        );
        assert_eq!(
            std::fs::read(f.store.root().join(OMISSIONS_FILE)).unwrap(),
            bytes
        );
        std::fs::write(f.directory.path().join("first.png"), b"CHANGED SOURCE").unwrap();
        assert!(!diagnose(f.store.root(), &f.file).unwrap()[0].omission_confirmed);
        assert!(crate::import_completion::verify_export(f.store.root(), &f.file).is_err());
        // Explicitly restoring later revokes the old omission and copies only current selected bytes.
        recover(f.store.root(), &f.file, &[key], &[]).unwrap();
        assert!(Omissions::read(&f.store).unwrap().attachments.is_empty());
        crate::import_completion::verify_export(f.store.root(), &f.file).unwrap();
    }

    #[test]
    fn malformed_or_unknown_requests_fail_before_any_selected_mutation() {
        let f = fixture();
        let key = remove(&f, 0);
        let before = snapshot(f.directory.path());
        assert!(
            recover(
                f.store.root(),
                &f.file,
                &[key.clone(), "unknown".into()],
                &[]
            )
            .is_err()
        );
        assert!(
            recover(
                f.store.root(),
                &f.file,
                std::slice::from_ref(&key),
                std::slice::from_ref(&key)
            )
            .is_err()
        );
        assert!(recover(f.store.root(), &f.file, &[], &[]).is_err());
        let present = diagnose(f.store.root(), &f.file).unwrap()[1]
            .selection
            .clone();
        assert!(recover(f.store.root(), &f.file, &[], &[present]).is_err());
        assert_eq!(snapshot(f.directory.path()), before);
        std::fs::write(f.store.root().join(OMISSIONS_FILE), b"{}").unwrap();
        assert!(diagnose(f.store.root(), &f.file).is_err());
    }

    #[test]
    fn missing_staged_sources_and_bundle_escapes_are_reported_without_modification() {
        let f = fixture();
        let key = remove(&f, 0);
        std::fs::remove_file(f.directory.path().join("first.png")).unwrap();
        let before = snapshot(f.directory.path());
        let rows = diagnose(f.store.root(), &f.file).unwrap();
        assert!(rows[0].source_error.is_some());
        assert!(rows[0].source_sha256.is_none());
        assert!(recover(f.store.root(), &f.file, std::slice::from_ref(&key), &[]).is_err());
        assert!(recover(f.store.root(), &f.file, &[], &[key]).is_err());
        assert_eq!(snapshot(f.directory.path()), before);
        let mut attachment = f.export.tickets[0].attachments[0].clone();
        attachment.stored_path = "../outside.png".into();
        assert!(source_path(f.directory.path(), &attachment).is_err());
        #[cfg(unix)]
        {
            let outside = tempfile::NamedTempFile::new().unwrap();
            std::os::unix::fs::symlink(outside.path(), f.directory.path().join("escape.png"))
                .unwrap();
            attachment.stored_path = "escape.png".into();
            assert!(source_path(f.directory.path(), &attachment).is_err());
        }
        attachment.stored_path = f.file.display().to_string();
        assert!(source_path(f.directory.path(), &attachment).is_err());
    }

    #[test]
    fn explicit_legacy_recovery_cannot_override_an_unfinished_checkpoint() {
        let f = fixture();
        let key = remove(&f, 0);
        let pending = f.store.root().join("hotsheet-hs1-import-pending");
        std::fs::create_dir_all(&pending).unwrap();
        std::fs::write(pending.join(format!("{}.json", f.id)), b"{}").unwrap();
        let before = snapshot(f.directory.path());
        assert!(diagnose(f.store.root(), &f.file).unwrap()[0].pending_import);
        assert!(recover(f.store.root(), &f.file, std::slice::from_ref(&key), &[]).is_err());
        assert!(recover(f.store.root(), &f.file, &[], &[key]).is_err());
        assert_eq!(snapshot(f.directory.path()), before);
    }

    #[test]
    fn inconsistent_omission_evidence_is_not_accepted() {
        let f = fixture();
        let key = remove(&f, 0);
        recover(f.store.root(), &f.file, &[], std::slice::from_ref(&key)).unwrap();
        let original = std::fs::read(f.store.root().join(OMISSIONS_FILE)).unwrap();
        for field in [
            "source_sha256",
            "source_filename",
            "ticket_id",
            "attachment_id",
        ] {
            let mut evidence = Omissions::read(&f.store).unwrap();
            let entry = evidence.attachments.get_mut(&key).unwrap();
            match field {
                "source_sha256" => entry.source_sha256 = "00".repeat(32),
                "source_filename" => entry.source_filename = "wrong.png".into(),
                "ticket_id" => entry.ticket_id = Ulid::new(),
                _ => entry.attachment_id = Ulid::new(),
            }
            evidence.write(&f.store).unwrap();
            assert!(!diagnose(f.store.root(), &f.file).unwrap()[0].omission_confirmed);
            assert!(crate::import_completion::verify_export(f.store.root(), &f.file).is_err());
            std::fs::write(f.store.root().join(OMISSIONS_FILE), &original).unwrap();
        }
    }

    #[test]
    fn changed_export_identity_cannot_reuse_an_omission() {
        let f = fixture();
        let key = remove(&f, 0);
        recover(f.store.root(), &f.file, &[], &[key]).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&f.file).unwrap()).unwrap();
        value["tickets"][0]["attachments"][0]["original_filename"] = "different.png".into();
        std::fs::write(&f.file, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(!diagnose(f.store.root(), &f.file).unwrap()[0].omission_confirmed);
        assert!(crate::import_completion::verify_export(f.store.root(), &f.file).is_err());
    }
}
