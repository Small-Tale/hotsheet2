//! HS2-Z8X4XP: the unsafe-attachment-name repair. Synthetic ticket content only.

use super::*;
use hotsheet_model::derive_slug;

const TICKET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const ATTACHMENT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FB0";
const OTHER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FB1";
const PAYLOAD: &[u8] = b"synthetic payload";
const OUTSIDE: &[u8] = b"outside secret";
const BACKSLASH: &str = "shots\\a.png";

fn ulid(value: &str) -> Ulid {
    Ulid::from_string(value).unwrap()
}

fn now() -> Timestamp {
    Timestamp::new("2026-11-26T00:00:00Z")
}

fn temp_store() -> (tempfile::TempDir, FsStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path().join("store"), &StoreMetadata::new("HS")).unwrap();
    (dir, store)
}

fn attachment(id: &str, filename: &str) -> Attachment {
    Attachment {
        id: ulid(id),
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

/// Write the ticket file directly, as a git pull from an older clone would.
fn sync_in(store: &FsStore, attachments: Vec<Attachment>) -> PathBuf {
    let id = ulid(TICKET);
    let mut ticket = Ticket::new(
        id,
        derive_slug(&id, "HS"),
        "Synced",
        "bug",
        "2026-08-19T00:00:00Z",
        "2026-08-19T00:00:00Z",
    );
    ticket.attachments = attachments;
    let path = store.ticket_path(&id);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, hotsheet_model::to_file_string(&ticket)).unwrap();
    path
}

fn nested(store: &FsStore, attachment: &str, name: &str) -> PathBuf {
    store
        .attachment_dir(&ulid(TICKET))
        .join(attachment)
        .join(name)
}

fn legacy(store: &FsStore, name: &str) -> PathBuf {
    store.attachment_dir(&ulid(TICKET)).join(name)
}

fn put(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn assert_corrupt_naming_the_fix(store: &FsStore) {
    let listing = store.list_tickets_resilient().unwrap();
    assert_eq!(listing.corrupt.len(), 1);
    assert!(
        listing.corrupt[0]
            .error
            .contains("hotsheet-cli doctor --repair-attachment-names"),
        "{}",
        listing.corrupt[0].error
    );
}

/// Repair, then prove the ticket reads, serves the kept payload, and a second run
/// changes nothing.
fn assert_repaired(store: &FsStore, ticket_path: &Path, expected: RepairedPayload) {
    let report = store.repair_unsafe_attachment_names(now()).unwrap();
    assert!(report.refused.is_empty(), "{report:?}");
    assert_eq!(report.repaired.len(), 1);
    let repaired = &report.repaired[0];
    assert_eq!(repaired.path, ticket_path);
    assert_eq!(repaired.renames.len(), 1);
    assert_eq!(
        repaired.renames[0],
        AttachmentRename {
            attachment_id: ulid(ATTACHMENT),
            from: BACKSLASH.into(),
            to: "a.png".into(),
            payload: expected,
        }
    );

    let ticket = store.read_ticket(&ulid(TICKET)).unwrap();
    assert_eq!(ticket.attachments.len(), 1, "no legacy twin was adopted");
    assert_eq!(ticket.attachments[0].filename, "a.png");
    assert_eq!(ticket.updated_at, now());
    let (_, bytes) = store
        .read_attachment(&ulid(TICKET), &ulid(ATTACHMENT))
        .unwrap();
    assert_eq!(bytes, PAYLOAD);
    assert_eq!(
        fs::read(nested(store, ATTACHMENT, "a.png")).unwrap(),
        PAYLOAD
    );
    assert!(store.list_tickets_resilient().unwrap().corrupt.is_empty());

    let file = fs::read(ticket_path).unwrap();
    let again = store.repair_unsafe_attachment_names(now()).unwrap();
    assert_eq!(again, AttachmentNameRepairReport::default());
    assert_eq!(fs::read(ticket_path).unwrap(), file, "a repeat is a no-op");
}

#[test]
fn hs2_z8x4xp_repairs_a_backslash_name_with_a_nested_payload() {
    let (_dir, store) = temp_store();
    let path = sync_in(&store, vec![attachment(ATTACHMENT, BACKSLASH)]);
    put(&nested(&store, ATTACHMENT, BACKSLASH), PAYLOAD);
    assert_corrupt_naming_the_fix(&store);

    assert_repaired(&store, &path, RepairedPayload::MovedFromNested);
    assert!(!nested(&store, ATTACHMENT, BACKSLASH).exists());
}

#[test]
fn hs2_z8x4xp_repairs_a_backslash_name_with_a_legacy_payload() {
    let (_dir, store) = temp_store();
    let path = sync_in(&store, vec![attachment(ATTACHMENT, BACKSLASH)]);
    put(&legacy(&store, BACKSLASH), PAYLOAD);
    assert_corrupt_naming_the_fix(&store);

    assert_repaired(&store, &path, RepairedPayload::MovedFromLegacy);
    assert!(!legacy(&store, BACKSLASH).exists());
    assert!(
        !legacy(&store, "a.png").exists(),
        "the payload moved to the nested layout"
    );
}

#[test]
fn hs2_z8x4xp_finishes_an_interrupted_repair_whose_payload_already_moved() {
    let (_dir, store) = temp_store();
    let path = sync_in(&store, vec![attachment(ATTACHMENT, BACKSLASH)]);
    put(&nested(&store, ATTACHMENT, "a.png"), PAYLOAD);

    assert_repaired(&store, &path, RepairedPayload::AlreadyInPlace);
}

#[test]
fn hs2_z8x4xp_renames_an_entry_whose_payload_is_missing() {
    let (_dir, store) = temp_store();
    sync_in(&store, vec![attachment(ATTACHMENT, BACKSLASH)]);

    let report = store.repair_unsafe_attachment_names(now()).unwrap();
    assert_eq!(
        report.repaired[0].renames[0].payload,
        RepairedPayload::NotFound
    );
    let ticket = store.read_ticket(&ulid(TICKET)).unwrap();
    assert_eq!(ticket.attachments[0].filename, "a.png");
}

/// Assert the repair refused the ticket and wrote nothing.
fn assert_refused(store: &FsStore, ticket_path: &Path, files: &[(PathBuf, &[u8])]) {
    let before = fs::read(ticket_path).unwrap();
    let report = store.repair_unsafe_attachment_names(now()).unwrap();
    assert!(report.repaired.is_empty(), "{report:?}");
    assert_eq!(report.refused.len(), 1);
    assert_eq!(report.refused[0].path, ticket_path);
    assert_eq!(report.refused[0].id, Some(ulid(TICKET)));
    assert!(report.refused[0].reason.contains("a.png"), "{report:?}");
    assert_eq!(fs::read(ticket_path).unwrap(), before);
    for (path, bytes) in files {
        assert_eq!(fs::read(path).unwrap(), *bytes, "{}", path.display());
    }
    assert_eq!(store.list_tickets_resilient().unwrap().corrupt.len(), 1);
}

#[test]
fn hs2_z8x4xp_refuses_a_name_that_collides_with_another_entry() {
    let (_dir, store) = temp_store();
    let path = sync_in(
        &store,
        vec![
            attachment(OTHER, "A.PNG"),
            attachment(ATTACHMENT, BACKSLASH),
        ],
    );
    let source = nested(&store, ATTACHMENT, BACKSLASH);
    let other = nested(&store, OTHER, "A.PNG");
    put(&source, PAYLOAD);
    put(&other, OUTSIDE);

    assert_refused(&store, &path, &[(source, PAYLOAD), (other, OUTSIDE)]);
}

#[test]
fn hs2_z8x4xp_refuses_two_unsafe_names_that_sanitize_alike() {
    let (_dir, store) = temp_store();
    let path = sync_in(
        &store,
        vec![
            attachment(OTHER, "old\\a.png"),
            attachment(ATTACHMENT, BACKSLASH),
        ],
    );
    assert_refused(&store, &path, &[]);
}

#[test]
fn hs2_z8x4xp_refuses_when_the_target_file_already_exists() {
    let (_dir, store) = temp_store();
    let path = sync_in(&store, vec![attachment(ATTACHMENT, BACKSLASH)]);
    let source = nested(&store, ATTACHMENT, BACKSLASH);
    let target = nested(&store, ATTACHMENT, "a.png");
    put(&source, PAYLOAD);
    put(&target, OUTSIDE);

    assert_refused(&store, &path, &[(source, PAYLOAD), (target, OUTSIDE)]);
}

#[test]
fn hs2_z8x4xp_refuses_when_a_legacy_file_would_become_a_twin() {
    let (_dir, store) = temp_store();
    let path = sync_in(&store, vec![attachment(ATTACHMENT, BACKSLASH)]);
    let source = nested(&store, ATTACHMENT, BACKSLASH);
    let twin = legacy(&store, "a.png");
    put(&source, PAYLOAD);
    put(&twin, OUTSIDE);

    assert_refused(&store, &path, &[(source, PAYLOAD), (twin, OUTSIDE)]);
}

/// Every file under `dir`, with its bytes.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        let metadata = fs::symlink_metadata(&path).unwrap();
        if metadata.is_dir() {
            found.extend(snapshot(&path));
        } else if metadata.is_file() {
            found.push((path.clone(), fs::read(&path).unwrap()));
        } else {
            found.push((path.clone(), Vec::new()));
        }
    }
    found.sort();
    found
}

#[test]
fn hs2_z8x4xp_traversal_names_are_renamed_without_touching_any_file() {
    let (dir, store) = temp_store();
    // Create the store's lock and ignore files first, so the snapshots compare only
    // what a repair touches.
    store.repair_unsafe_attachment_names(now()).unwrap();
    let absolute = dir.path().join("absolute-outside.txt");
    let cases = [
        ("../../outside.txt".to_string(), "outside.txt"),
        (format!("{ATTACHMENT}/../../../outside.txt"), "outside.txt"),
        (absolute.display().to_string(), "absolute-outside.txt"),
        ("..\\..\\outside.txt".to_string(), "outside.txt"),
        (format!("{ATTACHMENT}/../sibling.txt"), "sibling.txt"),
        ("..".to_string(), "attachment"),
        ("a\\..".to_string(), "attachment"),
    ];
    for (hostile, expected) in cases {
        let _ = fs::remove_dir_all(store.root().join("attachments"));
        put(&store.root().join("outside.txt"), OUTSIDE);
        put(&absolute, OUTSIDE);
        // An unrelated file inside the ticket's attachment directory stays put too.
        put(&legacy(&store, "sibling-keep.txt"), OUTSIDE);
        let path = sync_in(&store, vec![attachment(ATTACHMENT, &hostile)]);
        let outside_before = snapshot(dir.path());

        let report = store.repair_unsafe_attachment_names(now()).unwrap();
        assert!(report.refused.is_empty(), "{hostile:?}: {report:?}");
        let rename = &report.repaired[0].renames[0];
        assert_eq!(rename.to, expected, "{hostile:?}");
        assert_eq!(rename.payload, RepairedPayload::NotFound, "{hostile:?}");

        let mut after = snapshot(dir.path());
        let mut before = outside_before;
        // Only the ticket file itself was rewritten.
        after.retain(|(file, _)| file != &path);
        before.retain(|(file, _)| file != &path);
        assert_eq!(after, before, "{hostile:?} touched a file");
        let ticket = store.read_ticket(&ulid(TICKET)).unwrap();
        assert!(
            ticket
                .attachments
                .iter()
                .any(|item| item.id == ulid(ATTACHMENT) && item.filename == expected),
            "{hostile:?}: {:?}",
            ticket.attachments
        );
    }
}

#[cfg(unix)]
#[test]
fn hs2_z8x4xp_never_moves_a_symlinked_payload() {
    let (dir, store) = temp_store();
    let outside = dir.path().join("outside.txt");
    put(&outside, OUTSIDE);
    sync_in(&store, vec![attachment(ATTACHMENT, BACKSLASH)]);
    let link = nested(&store, ATTACHMENT, BACKSLASH);
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&outside, &link).unwrap();

    let report = store.repair_unsafe_attachment_names(now()).unwrap();
    assert_eq!(
        report.repaired[0].renames[0].payload,
        RepairedPayload::NotFound
    );
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!nested(&store, ATTACHMENT, "a.png").exists());
    assert_eq!(fs::read(&outside).unwrap(), OUTSIDE);
}

#[test]
fn hs2_z8x4xp_leaves_healthy_and_otherwise_corrupt_tickets_alone() {
    let (_dir, store) = temp_store();
    assert_eq!(
        store.repair_unsafe_attachment_names(now()).unwrap(),
        AttachmentNameRepairReport::default()
    );

    // A ticket corrupt for another reason only: never a candidate.
    let path = sync_in(&store, vec![attachment(ATTACHMENT, "a.png")]);
    let text = fs::read_to_string(&path).unwrap();
    let damaged = text.replace("<!-- hotsheet:body:end -->", "");
    fs::write(&path, &damaged).unwrap();
    assert_eq!(store.list_tickets_resilient().unwrap().corrupt.len(), 1);
    assert_eq!(
        store.repair_unsafe_attachment_names(now()).unwrap(),
        AttachmentNameRepairReport::default()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), damaged);

    // An unsafe name plus other damage: refused, nothing written or moved.
    let damaged = damaged.replace("filename: a.png", "filename: 'shots\\a.png'");
    fs::write(&path, &damaged).unwrap();
    put(&nested(&store, ATTACHMENT, BACKSLASH), PAYLOAD);
    let report = store.repair_unsafe_attachment_names(now()).unwrap();
    assert!(report.repaired.is_empty(), "{report:?}");
    assert_eq!(report.refused.len(), 1, "{report:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), damaged);
    assert_eq!(
        fs::read(nested(&store, ATTACHMENT, BACKSLASH)).unwrap(),
        PAYLOAD
    );
}
