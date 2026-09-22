//! Local proof that a completed HS1 import, including every payload, exists in Git.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use hotsheet_ticketing::FsStore;

const COMPLETION: &str = "hotsheet-hs1-import-completed.json";
const BACKUP: &str = "hotsheet-hs1-backup.json";

fn git(store: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(store)
        .args(args)
        .output()?;
    if !output.status.success() {
        bail!(
            "verifying HS1 import commit: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn proof_path(store: &Path, name: &str) -> Result<PathBuf> {
    Ok(store.join(git(store, &["rev-parse", "--git-path", name])?.trim()))
}

/// Explicit headless counterpart of the bridge's post-push verification. The same
/// versioned proof is consumed by reopened projects and authoritative DELETE checks.
pub fn verify_backup(store: &Path) -> Result<String> {
    let store = store.canonicalize()?;
    let completion_bytes = std::fs::read(proof_path(&store, COMPLETION)?)
        .context("retry HS1 migration before verifying its backup")?;
    let completion: serde_json::Value = serde_json::from_slice(&completion_bytes)?;
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(store.join(crate::HS1_IMPORT_RECEIPT))?)?;
    let revision = completion["revision"]
        .as_str()
        .context("missing completed import revision")?;
    let source = completion["sourceProject"]
        .as_str()
        .context("missing completed import project")?;
    if completion["version"] != 1
        || receipt["sourceProject"] != source
        || !(revision.len() == 40 || revision.len() == 64)
        || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("invalid HS1 completed-import proof; retry migration");
    }
    require_clean(&store)?;
    git(&store, &["merge-base", "--is-ancestor", revision, "HEAD"])?;
    let remote = git(&store, &["remote", "get-url", "origin"])?;
    let branch = git(&store, &["symbolic-ref", "HEAD"])?;
    let remote_heads = git(
        &store,
        &["ls-remote", "--exit-code", "origin", branch.trim()],
    )?;
    let remote_revision = remote_heads
        .lines()
        .find_map(|line| {
            let (oid, name) = line.split_once('\t')?;
            (name == branch.trim()).then_some(oid)
        })
        .context("the imported branch is not present on origin")?;
    git(
        &store,
        &["merge-base", "--is-ancestor", revision, remote_revision],
    )?;
    require_clean(&store)?;
    if std::fs::read(proof_path(&store, COMPLETION)?)? != completion_bytes
        || git(&store, &["remote", "get-url", "origin"])? != remote
    {
        bail!("HS1 import or origin changed during backup verification; retry verification");
    }
    let path = proof_path(&store, BACKUP)?;
    let mut staged =
        tempfile::NamedTempFile::new_in(path.parent().context("backup proof has no parent")?)?;
    serde_json::to_writer_pretty(
        &mut staged,
        &serde_json::json!({
            "version": 1, "sourceProject": source, "revision": revision,
            "remote": remote.trim(), "ref": branch.trim(), "remoteRevision": remote_revision,
        }),
    )?;
    staged.write_all(b"\n")?;
    staged.as_file().sync_all()?;
    staged.persist(path)?;
    Ok(revision.to_string())
}

/// A new attempt cannot inherit cleanup permission from an earlier import/push.
pub fn invalidate(store: &Path) -> Result<()> {
    if !store.join(".git").exists() {
        return Ok(());
    }
    for name in [COMPLETION, BACKUP] {
        match std::fs::remove_file(proof_path(store, name)?) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn require_clean(store: &Path) -> Result<()> {
    if !git(
        store,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?
    .is_empty()
    {
        bail!(
            "HS1 import is not fully committed; commit the imported ticket store and retry migration before backup or cleanup"
        );
    }
    let pending = store.join("hotsheet-hs1-import-pending");
    if pending.exists() && std::fs::read_dir(pending)?.next().is_some() {
        bail!("HS1 attachment import is still pending; retry migration before backup or cleanup");
    }
    Ok(())
}

/// Old partial imports can lack attachment metadata entirely. Validate against
/// the actual export as well as the destination's own metadata before any proof.
pub fn verify_export(store: &Path, export_path: &Path) -> Result<()> {
    let export: crate::import::ExportFile = serde_json::from_slice(&std::fs::read(export_path)?)?;
    let store = FsStore::open(store)?;
    for (index, source) in export.tickets.iter().enumerate() {
        let id = crate::import::import_id(&export.project, source, index);
        store.read_ticket(&id)?;
        for attachment in &source.attachments {
            let attachment_id = crate::import::attachment_id(&id, attachment);
            store.read_attachment(&id, &attachment_id).with_context(|| format!(
                "exported HS1 attachment {attachment_id} is missing from ticket {id}; recover the missing import files before backup or cleanup"
            ))?;
        }
    }
    Ok(())
}

/// Best-effort Git warnings are insufficient here: cleanup requires the exact
/// committed revision and all referenced ticket/attachment files in its tree.
pub fn record(store_path: &Path, source: &Path) -> Result<()> {
    let store_path = store_path.canonicalize()?;
    let revision = git(&store_path, &["rev-parse", "--verify", "HEAD"])?;
    require_clean(&store_path)?;
    let tree = git(
        &store_path,
        &["ls-tree", "-r", "-z", "--name-only", revision.trim()],
    )?;
    let committed: HashSet<&str> = tree.split('\0').filter(|path| !path.is_empty()).collect();
    let store = FsStore::open(&store_path)?;
    let mut required = vec![
        store_path.join(crate::HS1_IMPORT_RECEIPT),
        store_path.join("hotsheet-store.json"),
    ];
    for ticket in store.list_tickets()? {
        required.push(store.ticket_path(&ticket.id));
        for attachment in ticket.attachments {
            store
                .read_attachment(&ticket.id, &attachment.id)
                .with_context(|| format!("verifying HS1 attachment {}", attachment.id))?;
            let directory = store.attachment_dir(&ticket.id);
            let nested = directory
                .join(attachment.id.to_string())
                .join(&attachment.filename);
            required.push(if nested.is_file() {
                nested
            } else {
                directory.join(&attachment.filename)
            });
        }
    }
    for path in required {
        let relative = path
            .strip_prefix(&store_path)?
            .to_string_lossy()
            .replace('\\', "/");
        if !committed.contains(relative.as_str()) {
            bail!(
                "HS1 import file is not included in commit {}: {relative}",
                revision.trim()
            );
        }
    }
    require_clean(&store_path)?;
    if git(&store_path, &["rev-parse", "--verify", "HEAD"])? != revision {
        bail!("ticket-store revision changed while verifying HS1 import; retry migration");
    }
    let path = proof_path(&store_path, COMPLETION)?;
    let mut staged =
        tempfile::NamedTempFile::new_in(path.parent().context("import proof has no parent")?)?;
    serde_json::to_writer_pretty(
        &mut staged,
        &serde_json::json!({
            "version": 1, "sourceProject": source, "revision": revision.trim(),
        }),
    )?;
    staged.write_all(b"\n")?;
    staged.as_file().sync_all()?;
    staged.persist(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_model::{Ticket, Ulid};
    use hotsheet_ticketing::StoreMetadata;

    fn fixture() -> (tempfile::TempDir, FsStore, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let store = FsStore::init(directory.path(), &StoreMetadata::new("HS")).unwrap();
        git(store.root(), &["init", "--quiet"]).unwrap();
        git(store.root(), &["config", "user.name", "Hot Sheet test"]).unwrap();
        git(
            store.root(),
            &["config", "user.email", "hotsheet@example.invalid"],
        )
        .unwrap();
        let id = Ulid::new();
        store
            .write_ticket(&Ticket::new(
                id,
                "HS-TEST",
                "Imported",
                "task",
                "2026-09-22T00:00:00Z",
                "2026-09-22T00:00:00Z",
            ))
            .unwrap();
        let (_, payload) = store
            .write_attachment(
                &id,
                Ulid::new(),
                "2026-09-22T00:00:00Z".into(),
                "shot.png",
                b"PAYLOAD",
            )
            .unwrap();
        std::fs::write(
            store.root().join(crate::HS1_IMPORT_RECEIPT),
            "{\"sourceProject\":\"/work/demo\"}\n",
        )
        .unwrap();
        commit(store.root());
        (directory, store, payload)
    }

    fn commit(root: &Path) {
        git(root, &["add", "-A"]).unwrap();
        git(root, &["commit", "--quiet", "-m", "Fixture"]).unwrap();
    }

    #[test]
    fn export_verification_detects_missing_metadata_and_never_restores_intentional_deletions() {
        let directory = tempfile::tempdir().unwrap();
        let store = FsStore::init(directory.path(), &StoreMetadata::new("HS")).unwrap();
        let source = serde_json::json!({"project":{"name":"Demo"},"tickets":[{
            "ticket_number":"HS-1","title":"Imported",
            "attachments":[{"original_filename":"shot.png","stored_path":"shot.png"}]
        }]});
        let export_path = directory.path().join("source.json");
        std::fs::write(&export_path, serde_json::to_vec(&source).unwrap()).unwrap();
        let export: crate::import::ExportFile = serde_json::from_value(source).unwrap();
        let id = crate::import::import_id(&export.project, &export.tickets[0], 0);
        store
            .write_ticket(&Ticket::new(
                id,
                "HS-TEST",
                "User edited",
                "task",
                "2026-09-22T00:00:00Z",
                "2026-09-22T00:00:00Z",
            ))
            .unwrap();
        assert!(
            verify_export(store.root(), &export_path)
                .unwrap_err()
                .to_string()
                .contains("exported HS1 attachment")
        );
        let attachment_id = crate::import::attachment_id(&id, &export.tickets[0].attachments[0]);
        store
            .write_attachment(
                &id,
                attachment_id,
                "2026-09-22T00:00:00Z".into(),
                "renamed.png",
                b"USER CONTENT",
            )
            .unwrap();
        verify_export(store.root(), &export_path).unwrap();
        let deleted = store
            .remove_attachment(&id, &attachment_id, "2026-09-22T01:00:00Z".into())
            .unwrap();
        assert!(verify_export(store.root(), &export_path).is_err());
        assert_eq!(store.read_ticket(&id).unwrap(), deleted);
    }

    #[test]
    fn completion_proves_exact_clean_revision_and_new_attempt_invalidates_it() {
        let (_directory, store, _payload) = fixture();
        record(store.root(), Path::new("/work/demo")).unwrap();
        let path = proof_path(store.root(), COMPLETION).unwrap();
        let proof: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            proof["revision"],
            git(store.root(), &["rev-parse", "HEAD"]).unwrap().trim()
        );
        assert_eq!(proof["sourceProject"], "/work/demo");
        assert!(
            git(store.root(), &["status", "--porcelain"])
                .unwrap()
                .is_empty()
        );
        let backup = proof_path(store.root(), BACKUP).unwrap();
        std::fs::write(&backup, "{}").unwrap();
        invalidate(store.root()).unwrap();
        assert!(!path.exists());
        assert!(!backup.exists());
    }

    #[test]
    fn completion_rejects_ignored_payloads_even_when_git_status_is_clean() {
        let (_directory, store, payload) = fixture();
        let relative = payload
            .strip_prefix(store.root())
            .unwrap()
            .to_str()
            .unwrap();
        git(store.root(), &["rm", "--cached", relative]).unwrap();
        let ignore = store.root().join(".gitignore");
        let original = std::fs::read_to_string(&ignore).unwrap();
        std::fs::write(ignore, format!("{original}\n/attachments/\n")).unwrap();
        commit(store.root());
        assert!(
            git(store.root(), &["status", "--porcelain"])
                .unwrap()
                .is_empty()
        );
        assert!(
            record(store.root(), Path::new("/work/demo"))
                .unwrap_err()
                .to_string()
                .contains("not included in commit")
        );
        assert!(!proof_path(store.root(), COMPLETION).unwrap().exists());
        std::fs::remove_file(payload).unwrap();
        let error = record(store.root(), Path::new("/work/demo")).unwrap_err();
        assert!(
            error.to_string().contains("verifying HS1 attachment"),
            "{error:#}"
        );
    }

    #[test]
    fn completion_rejects_uncommitted_files_and_pending_attachment_checkpoints() {
        let (_directory, store, _payload) = fixture();
        std::fs::write(store.root().join("uncommitted.txt"), "not backed up").unwrap();
        assert!(
            record(store.root(), Path::new("/work/demo"))
                .unwrap_err()
                .to_string()
                .contains("not fully committed")
        );
        std::fs::remove_file(store.root().join("uncommitted.txt")).unwrap();
        let pending = store.root().join("hotsheet-hs1-import-pending");
        std::fs::create_dir(&pending).unwrap();
        std::fs::write(pending.join("ticket.json"), "{}").unwrap();
        commit(store.root());
        assert!(
            record(store.root(), Path::new("/work/demo"))
                .unwrap_err()
                .to_string()
                .contains("still pending")
        );
        assert!(!proof_path(store.root(), COMPLETION).unwrap().exists());
    }
}
