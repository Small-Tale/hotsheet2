//! Import a portable `hotsheet-export.json` (produced by the Node HS1 exporter,
//! `docs/07-migration.md` §7.2.1 shape B) into a git store, writing ticket files
//! through the core's own writer so the format never drifts.
//!
//! Idempotent: tickets whose `legacy_number` is already present are skipped.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use hotsheet_model::{
    CloseReason, Note, NoteKind, Priority, Status, Ticket, Timestamp, Ulid, derive_slug,
};
use hotsheet_ticketing::FsStore;
use serde::Deserialize;

/// The export-file `exportVersion` this importer understands (`docs/07` §7.2.1).
pub const SUPPORTED_EXPORT_VERSION: u32 = 1;

/// A parsed export bundle (`docs/07` §7.2.1).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFile {
    #[serde(default)]
    pub export_version: u32,
    #[serde(default)]
    pub project: ProjectInfo,
    #[serde(default)]
    pub tickets: Vec<ExportTicket>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub name: Option<String>,
    pub ticket_prefix: Option<String>,
}

/// One HS1 ticket as exported (snake_case, mirroring the HS1 schema).
#[derive(Debug, Deserialize)]
pub struct ExportTicket {
    pub ticket_number: Option<String>,
    pub title: String,
    #[serde(default)]
    pub details: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub up_next: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Vec<ExportNote>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub completed_at: Option<String>,
    pub verified_at: Option<String>,
    #[serde(default)]
    pub attachments: Vec<ExportAttachment>,
}

#[derive(Debug, Deserialize)]
pub struct ExportNote {
    #[serde(default)]
    pub id: Option<String>,
    pub text: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// An attachment as exported: its display filename + the staged file path (relative
/// to the export JSON's directory, written by the migrator's staging pass).
#[derive(Debug, Deserialize)]
pub struct ExportAttachment {
    pub original_filename: Option<String>,
    pub stored_path: String,
}

/// Result of an import run.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub written: usize,
    pub skipped: usize,
    pub attachments: usize,
}

/// Import `export` into `store`. Two passes: assign every source ticket a fresh ULID
/// first, then write files with `blocked_by` remapped from old `HS-N` refs to the new
/// ULIDs. HS1 note ids (`n_…`) are replaced with fresh ULIDs so appends merge (§2.6).
///
/// Attachment `stored_path`s are resolved relative to `base_dir` (the export JSON's
/// directory, where the migrator staged the files) and copied into the store.
pub fn import(store: &FsStore, export: &ExportFile, base_dir: &Path) -> Result<ImportSummary> {
    let prefix = store.metadata()?.ticket_prefix;

    let already: HashSet<String> = store
        .list_tickets()?
        .into_iter()
        .filter_map(|t| t.legacy_number)
        .collect();

    // Pass 1 — one new ULID per source ticket, keyed by its old number for edges.
    let ids: Vec<Ulid> = export.tickets.iter().map(|_| Ulid::new()).collect();
    let id_by_number: HashMap<&str, Ulid> = export
        .tickets
        .iter()
        .zip(&ids)
        .filter_map(|(t, id)| t.ticket_number.as_deref().map(|n| (n, *id)))
        .collect();

    // Pass 2 — build + write.
    let mut summary = ImportSummary::default();
    for (src, id) in export.tickets.iter().zip(&ids) {
        if let Some(num) = &src.ticket_number {
            if already.contains(num) {
                summary.skipped += 1;
                continue;
            }
        }
        store.write_ticket(&build_ticket(src, *id, &prefix, &id_by_number))?;
        summary.written += 1;
        summary.attachments += copy_attachments(store, base_dir, id, &src.attachments)?;
    }
    Ok(summary)
}

/// Copy a ticket's staged attachment files into `attachments/<new-ulid>/`.
fn copy_attachments(
    store: &FsStore,
    base_dir: &Path,
    id: &Ulid,
    attachments: &[ExportAttachment],
) -> Result<usize> {
    let mut n = 0;
    for att in attachments {
        let src = base_dir.join(&att.stored_path);
        let bytes = std::fs::read(&src)
            .with_context(|| format!("reading staged attachment {}", src.display()))?;
        let filename = att
            .original_filename
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&att.stored_path);
        store.write_attachment(id, filename, &bytes)?;
        n += 1;
    }
    Ok(n)
}

fn build_ticket(
    src: &ExportTicket,
    id: Ulid,
    prefix: &str,
    id_by_number: &HashMap<&str, Ulid>,
) -> Ticket {
    let created = src.created_at.clone().unwrap_or_default();
    let updated = src.updated_at.clone().unwrap_or_else(|| created.clone());

    let mut t = Ticket::new(
        id,
        derive_slug(&id, prefix),
        src.title.clone(),
        src.category.clone().unwrap_or_else(|| "issue".to_string()),
        created,
        updated,
    );
    t.priority = parse_priority(src.priority.as_deref());
    t.status = parse_status(src.status.as_deref());
    t.up_next = src.up_next;
    t.tags = src.tags.clone();
    t.details = src.details.clone().unwrap_or_default();
    t.completed_at = src.completed_at.clone().map(Timestamp::from);
    t.verified_at = src.verified_at.clone().map(Timestamp::from);
    t.legacy_number = src.ticket_number.clone();

    // A completed/verified HS1 ticket carries a `completed` close outcome (docs/07).
    if matches!(t.status, Status::Completed | Status::Verified) {
        t.closed_at = t.completed_at.clone().or_else(|| t.verified_at.clone());
        t.close_reason = Some(CloseReason::Completed);
    }

    // Remap dependency edges; drop refs to tickets outside this export.
    t.blocked_by = src
        .blocked_by
        .iter()
        .filter_map(|n| id_by_number.get(n.as_str()).copied())
        .collect();

    t.notes = src
        .notes
        .iter()
        .map(|n| Note {
            id: n
                .id
                .as_deref()
                .and_then(|s| Ulid::from_string(s).ok())
                .unwrap_or_else(Ulid::new),
            kind: NoteKind::Regular,
            at: Timestamp::new(n.created_at.clone().unwrap_or_default()),
            text: n.text.clone(),
        })
        .collect();

    t
}

fn parse_priority(s: Option<&str>) -> Priority {
    match s {
        Some("highest") => Priority::Highest,
        Some("high") => Priority::High,
        Some("low") => Priority::Low,
        Some("lowest") => Priority::Lowest,
        _ => Priority::Default,
    }
}

fn parse_status(s: Option<&str>) -> Status {
    match s {
        Some("started") => Status::Started,
        Some("completed") => Status::Completed,
        Some("verified") => Status::Verified,
        Some("backlog") => Status::Backlog,
        Some("archive") => Status::Archive,
        Some("deleted") => Status::Deleted,
        Some("moved") => Status::Moved,
        _ => Status::NotStarted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_ticketing::StoreMetadata;

    fn temp_store() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        (dir, store)
    }

    fn export_json() -> ExportFile {
        let json = r#"{
          "exportVersion": 1,
          "project": { "name": "Demo", "ticketPrefix": "HS" },
          "tickets": [
            {
              "ticket_number": "HS-1200", "title": "Root cause",
              "category": "bug", "priority": "high", "status": "completed",
              "up_next": false, "tags": ["ui"],
              "notes": [{ "id": "n_abc", "text": "done", "created_at": "2026-08-01T00:00:00Z" }],
              "blocked_by": [],
              "created_at": "2026-08-01T00:00:00Z", "updated_at": "2026-08-02T00:00:00Z",
              "completed_at": "2026-08-02T00:00:00Z", "verified_at": null
            },
            {
              "ticket_number": "HS-1234", "title": "Depends on 1200",
              "category": "feature", "priority": "default", "status": "started",
              "up_next": true, "tags": [],
              "notes": [],
              "blocked_by": ["HS-1200", "HS-9999"],
              "created_at": "2026-08-03T00:00:00Z", "updated_at": "2026-08-03T00:00:00Z",
              "completed_at": null, "verified_at": null
            }
          ]
        }"#;
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn imports_tickets_and_remaps_edges() {
        let (_dir, store) = temp_store();
        let summary = import(&store, &export_json(), Path::new(".")).unwrap();
        assert_eq!(
            summary,
            ImportSummary {
                written: 2,
                skipped: 0,
                attachments: 0,
            }
        );

        let tickets = store.list_tickets().unwrap();
        assert_eq!(tickets.len(), 2);

        let by_legacy = |num: &str| {
            tickets
                .iter()
                .find(|t| t.legacy_number.as_deref() == Some(num))
                .cloned()
                .unwrap()
        };
        let root = by_legacy("HS-1200");
        let dep = by_legacy("HS-1234");

        // Legacy number preserved; a fresh ULID slug is the live handle.
        assert!(root.slug.starts_with("HS-"));
        assert_ne!(root.slug, "HS-1200");

        // completed → close outcome mapped.
        assert_eq!(root.status, Status::Completed);
        assert_eq!(root.close_reason, Some(CloseReason::Completed));
        assert_eq!(
            root.closed_at.as_ref().map(Timestamp::as_str),
            Some("2026-08-02T00:00:00Z")
        );

        // The HS1 note id (n_abc) was replaced with a real ULID.
        assert_eq!(root.notes.len(), 1);
        assert_eq!(root.notes[0].text, "done");

        // blocked_by: HS-1200 remaps to root's ULID; the out-of-export HS-9999 is dropped.
        assert_eq!(dep.blocked_by, vec![root.id]);
        assert!(dep.up_next);
    }

    #[test]
    fn import_is_idempotent_by_legacy_number() {
        let (_dir, store) = temp_store();
        import(&store, &export_json(), Path::new(".")).unwrap();
        let again = import(&store, &export_json(), Path::new(".")).unwrap();
        assert_eq!(
            again,
            ImportSummary {
                written: 0,
                skipped: 2,
                attachments: 0,
            }
        );
        assert_eq!(store.list_tickets().unwrap().len(), 2, "no duplicates");
    }

    #[test]
    fn imports_staged_attachments_into_the_store() {
        let (_dir, store) = temp_store();
        // A staging dir (stands in for the export JSON's directory) with one file.
        let staging = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(staging.path().join("attachments/0")).unwrap();
        std::fs::write(staging.path().join("attachments/0/shot.png"), b"PNGDATA").unwrap();

        let json = r#"{
          "exportVersion": 1,
          "project": { "ticketPrefix": "HS" },
          "tickets": [
            { "ticket_number": "HS-1", "title": "with attachment",
              "notes": [], "blocked_by": [],
              "attachments": [
                { "original_filename": "shot.png", "stored_path": "attachments/0/shot.png" }
              ] }
          ]
        }"#;
        let export: ExportFile = serde_json::from_str(json).unwrap();
        let summary = import(&store, &export, staging.path()).unwrap();
        assert_eq!(summary.written, 1);
        assert_eq!(summary.attachments, 1);

        let ticket = &store.list_tickets().unwrap()[0];
        let file = store.attachment_dir(&ticket.id).join("shot.png");
        assert_eq!(std::fs::read(file).unwrap(), b"PNGDATA");
    }
}
