//! HS2-SY15VS: attachment filenames in synced ticket frontmatter are untrusted. A
//! ticket whose attachment names a parent directory, an absolute path, or a separator
//! must not read, remove or rename anything outside its attachment directory.
//! Synthetic ticket content only.

use super::*;
use hotsheet_model::derive_slug;

const TICKET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const ATTACHMENT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FB0";
const OUTSIDE: &[u8] = b"outside secret";

fn ulid(value: &str) -> Ulid {
    Ulid::from_string(value).unwrap()
}

fn temp_store() -> (tempfile::TempDir, FsStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path().join("store"), &StoreMetadata::new("HS")).unwrap();
    (dir, store)
}

fn attachment(filename: &str) -> Attachment {
    Attachment {
        id: ulid(ATTACHMENT),
        filename: filename.into(),
        created_at: Timestamp::new("2026-08-19T00:00:00Z"),
        batch_id: None,
        batch_label: None,
        actor: None,
        purpose: None,
        annotations: Vec::new(),
        crop: None,
    }
}

/// Write the ticket file directly, as a git pull from another clone would, bypassing
/// every store writer.
fn sync_in(store: &FsStore, filename: &str) -> Ulid {
    let id = ulid(TICKET);
    let mut ticket = Ticket::new(
        id,
        derive_slug(&id, "HS"),
        "Synced",
        "bug",
        "2026-08-19T00:00:00Z",
        "2026-08-19T00:00:00Z",
    );
    ticket.attachments.push(attachment(filename));
    let path = store.ticket_path(&id);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, hotsheet_model::to_file_string(&ticket)).unwrap();
    id
}

/// Outside targets for each hostile name, written before the operation runs.
fn hostile_cases(dir: &Path, store: &FsStore) -> Vec<(String, PathBuf)> {
    let root_outside = store.root().join("outside.txt");
    let tmp_outside = dir.join("absolute-outside.txt");
    let nested_outside = store.attachment_dir(&ulid(TICKET)).join("sibling.txt");
    vec![
        ("../../outside.txt".into(), root_outside.clone()),
        (
            format!("{ATTACHMENT}/../../../outside.txt"),
            root_outside.clone(),
        ),
        (tmp_outside.display().to_string(), tmp_outside),
        ("..\\..\\outside.txt".into(), root_outside),
        (format!("{ATTACHMENT}/../sibling.txt"), nested_outside),
        ("..".into(), store.attachment_dir(&ulid(TICKET))),
    ]
}

fn plant(target: &Path) {
    if target.is_dir() || target.extension().is_none() {
        fs::create_dir_all(target).unwrap();
        fs::write(target.join("keep.txt"), OUTSIDE).unwrap();
    } else {
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, OUTSIDE).unwrap();
    }
}

fn intact(target: &Path) -> bool {
    let file = if target.extension().is_none() {
        target.join("keep.txt")
    } else {
        target.to_path_buf()
    };
    fs::read(file).is_ok_and(|bytes| bytes == OUTSIDE)
}

#[test]
fn hs2_sy15vs_synced_unsafe_filename_never_reads_removes_or_renames_outside() {
    let (dir, store) = temp_store();
    for (name, target) in hostile_cases(dir.path(), &store) {
        plant(&target);
        let id = sync_in(&store, &name);
        let attachment_id = ulid(ATTACHMENT);

        let error = store.read_ticket(&id).unwrap_err();
        assert!(
            error.to_string().contains("unsafe filename"),
            "{name:?}: {error}"
        );
        assert!(
            store.read_attachment(&id, &attachment_id).is_err(),
            "{name:?} read"
        );
        assert!(
            store
                .remove_attachment(&id, &attachment_id, Timestamp::new("2026-08-20T00:00:00Z"))
                .is_err(),
            "{name:?} remove"
        );
        assert!(
            store
                .rename_attachment(
                    &id,
                    &attachment_id,
                    Timestamp::new("2026-08-20T00:00:00Z"),
                    "pulled-in.txt",
                )
                .is_err(),
            "{name:?} rename"
        );
        assert!(intact(&target), "{name:?} touched {}", target.display());
        assert!(
            !store
                .attachment_dir(&id)
                .join(ATTACHMENT)
                .join("pulled-in.txt")
                .exists(),
            "{name:?} pulled an outside file into the store"
        );

        // The refused ticket stays on disk and is reported, never silently dropped.
        let listing = store.list_tickets_resilient().unwrap();
        assert!(listing.tickets.is_empty(), "{name:?}");
        assert_eq!(listing.corrupt.len(), 1, "{name:?}");
        assert_eq!(listing.corrupt[0].id, Some(id));
        assert!(listing.corrupt[0].error.contains("unsafe filename"));
        assert!(
            fs::read_to_string(store.ticket_path(&id))
                .unwrap()
                .contains("filename:")
        );
    }
}

#[test]
fn hs2_sy15vs_payload_path_join_asserts_one_safe_component() {
    let (dir, store) = temp_store();
    let ticket_id = ulid(TICKET);
    let attachment_id = ulid(ATTACHMENT);
    for (name, _) in hostile_cases(dir.path(), &store) {
        let error = store
            .attachment_payload_path(&ticket_id, &attachment_id, &name)
            .unwrap_err();
        assert!(
            matches!(error, StoreError::UnsafeAttachmentFilename { attachment_id: id, .. } if id == attachment_id),
            "{name:?}: {error}"
        );
    }
    let legacy = store
        .attachment_payload_path(&ticket_id, &attachment_id, "proof.txt")
        .unwrap();
    assert_eq!(legacy, store.attachment_dir(&ticket_id).join("proof.txt"));
    let nested = store.attachment_dir(&ticket_id).join(ATTACHMENT);
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("proof.txt"), b"proof").unwrap();
    assert_eq!(
        store
            .attachment_payload_path(&ticket_id, &attachment_id, "proof.txt")
            .unwrap(),
        nested.join("proof.txt")
    );
}

#[test]
fn hs2_sy15vs_writers_store_only_names_the_parser_accepts() {
    let (_dir, store) = temp_store();
    let id = ulid(TICKET);
    store
        .write_ticket(&Ticket::new(
            id,
            derive_slug(&id, "HS"),
            "Uploads",
            "bug",
            "2026-08-19T00:00:00Z",
            "2026-08-19T00:00:00Z",
        ))
        .unwrap();
    let cases = [
        ("..\\..\\evil.txt", "evil.txt"),
        ("C:\\Users\\me\\shot.png", "shot.png"),
        ("../../nested/proof.txt", "proof.txt"),
        ("/etc/hosts", "hosts"),
        ("..", "attachment"),
        ("dir/", "attachment (2)"),
    ];
    for (index, (input, expected)) in cases.into_iter().enumerate() {
        let attachment_id = Ulid::from_parts(1, index as u128);
        let (ticket, path) = store
            .write_attachment(
                &id,
                attachment_id,
                Timestamp::new("2026-08-19T00:00:00Z"),
                input,
                b"bytes",
            )
            .unwrap();
        let stored = ticket
            .attachments
            .iter()
            .find(|attachment| attachment.id == attachment_id)
            .unwrap();
        assert_eq!(stored.filename, expected, "{input:?}");
        assert!(path.starts_with(store.attachment_dir(&id).join(attachment_id.to_string())));
    }
    // Every stored name round-trips through the parser.
    assert_eq!(store.read_ticket(&id).unwrap().attachments.len(), 6);
    assert_eq!(
        AtomicAttachment {
            id: ulid(ATTACHMENT),
            filename: "..\\..\\evil.txt".into(),
            created_at: Timestamp::new("2026-08-19T00:00:00Z"),
            bytes: Vec::new(),
        }
        .sanitized_filename(),
        "evil.txt"
    );
}

#[test]
fn hs2_sy15vs_legacy_discovery_skips_names_the_parser_refuses() {
    let (_dir, store) = temp_store();
    let id = ulid(TICKET);
    store
        .write_ticket(&Ticket::new(
            id,
            derive_slug(&id, "HS"),
            "Legacy",
            "bug",
            "2026-08-19T00:00:00Z",
            "2026-08-19T00:00:00Z",
        ))
        .unwrap();
    let dir = store.attachment_dir(&id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("legacy.txt"), b"old").unwrap();
    #[cfg(unix)]
    fs::write(dir.join("back\\slash.txt"), b"odd").unwrap();
    let names = store
        .read_ticket(&id)
        .unwrap()
        .attachments
        .into_iter()
        .map(|attachment| attachment.filename)
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["legacy.txt".to_string()]);
}
