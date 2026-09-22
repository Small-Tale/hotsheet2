//! Actual CLI + Git boundary coverage for explicit legacy attachment recovery.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use hotsheet_cli::import_recovery::{OMISSIONS_FILE, diagnose};
use hotsheet_ticketing::FsStore;
use predicates::prelude::*;

fn cli(store: &Path) -> Command {
    let mut command = Command::cargo_bin("hotsheet-cli").unwrap();
    command
        .env("GIT_AUTHOR_NAME", "Hot Sheet test")
        .env("GIT_AUTHOR_EMAIL", "hotsheet@example.invalid")
        .env("GIT_COMMITTER_NAME", "Hot Sheet test")
        .env("GIT_COMMITTER_EMAIL", "hotsheet@example.invalid")
        .arg("-C")
        .arg(store);
    command
}
fn git(store: &Path, args: &[&str]) -> String {
    let result = std::process::Command::new("git")
        .arg("-C")
        .arg(store)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Hot Sheet test")
        .env("GIT_AUTHOR_EMAIL", "hotsheet@example.invalid")
        .env("GIT_COMMITTER_NAME", "Hot Sheet test")
        .env("GIT_COMMITTER_EMAIL", "hotsheet@example.invalid")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().into()
}
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("store");
    let file = root.path().join("export.json");
    let value = serde_json::json!({"exportVersion":1,"project":{"name":"Legacy","ticketPrefix":"HS2"},"tickets":[{
        "ticket_number":"HS-1","title":"Original","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-02T00:00:00Z",
        "attachments":[{"original_filename":"first.txt","stored_path":"first.txt"},{"original_filename":"second.txt","stored_path":"second.txt"}]
    }]});
    std::fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
    std::fs::write(root.path().join("first.txt"), b"FIRST").unwrap();
    std::fs::write(root.path().join("second.txt"), b"SECOND").unwrap();
    (root, store, file)
}

#[test]
fn cli_diagnoses_without_writes_and_repairs_only_selected_legacy_payloads() {
    let (_root, store_path, file) = fixture();
    cli(&store_path).arg("import").arg(&file).assert().success();
    let store = FsStore::open(&store_path).unwrap().with_deferred_push();
    let rows = diagnose(&store_path, &file).unwrap();
    let id = rows[0].ticket_id;
    let first = rows[0].attachment_id;
    let second = rows[1].attachment_id;
    let mut ticket = store
        .rename_attachment(&id, &first, "2026-02-01T00:00:00Z".into(), "edited.txt")
        .unwrap();
    ticket.title = "User title".into();
    ticket.details = "Preserve details".into();
    store.write_ticket(&ticket).unwrap();
    store
        .remove_attachment(&id, &second, "2026-02-02T00:00:00Z".into())
        .unwrap();
    let before = store.read_ticket(&id).unwrap();
    std::fs::remove_file(
        store
            .attachment_dir(&id)
            .join(first.to_string())
            .join("edited.txt"),
    )
    .unwrap();
    git(&store_path, &["add", "-A"]);
    git(
        &store_path,
        &["commit", "-m", "Simulate legacy missing payload"],
    );
    // A normal reimport preserves the ambiguous legacy state.
    cli(&store_path).arg("import").arg(&file).assert().success();
    assert_eq!(store.read_ticket(&id).unwrap(), before);
    let head = git(&store_path, &["rev-parse", "HEAD"]);
    let ignore = store_path.join(".gitignore");
    let ignore_bytes = std::fs::read(&ignore).unwrap();
    std::fs::remove_file(&ignore).unwrap();
    let status = git(&store_path, &["status", "--porcelain"]);
    let output = cli(&store_path)
        .arg("import")
        .arg(&file)
        .arg("--diagnose-attachments")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report[0]["state"], "missing_payload");
    assert_eq!(report[1]["state"], "missing_metadata");
    assert!(!ignore.exists());
    assert_eq!(git(&store_path, &["status", "--porcelain"]), status);
    assert_eq!(git(&store_path, &["rev-parse", "HEAD"]), head);
    std::fs::write(&ignore, ignore_bytes).unwrap();
    let selected = report[0]["selection"].as_str().unwrap();
    cli(&store_path)
        .arg("import")
        .arg(&file)
        .args(["--restore-attachment", selected])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"restored\": 1"));
    assert_eq!(store.read_ticket(&id).unwrap(), before);
    assert_eq!(store.read_attachment(&id, &first).unwrap().1, b"FIRST");
    assert!(store.read_attachment(&id, &second).is_err());
    let repaired = git(&store_path, &["rev-parse", "HEAD"]);
    cli(&store_path)
        .arg("import")
        .arg(&file)
        .args(["--restore-attachment", selected])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"unchanged\": 1"));
    assert_eq!(git(&store_path, &["rev-parse", "HEAD"]), repaired);
    assert!(git(&store_path, &["status", "--porcelain"]).is_empty());
}

#[test]
fn cli_explicit_omission_can_be_verified_and_backed_up_after_intentional_deletion() {
    let (root, store_path, file) = fixture();
    let source = root.path().join("project/.hotsheet");
    std::fs::create_dir_all(&source).unwrap();
    let exporter = root.path().join("export.mjs");
    let bundle = serde_json::to_string(root.path().to_str().unwrap()).unwrap();
    std::fs::write(&exporter, format!(r#"import {{copyFileSync}} from 'node:fs';
import {{dirname,join}} from 'node:path';
const args=process.argv.slice(2),out=args[args.indexOf('--out')+1],bundle={bundle};
copyFileSync(join(bundle,'export.json'),out);
for(const file of ['first.txt','second.txt']) copyFileSync(join(bundle,file),join(dirname(out),file));
"#)).unwrap();
    let migrate = || {
        let mut command = Command::cargo_bin("hotsheet-migrate").unwrap();
        command
            .env("GIT_AUTHOR_NAME", "Hot Sheet test")
            .env("GIT_AUTHOR_EMAIL", "hotsheet@example.invalid")
            .env("GIT_COMMITTER_NAME", "Hot Sheet test")
            .env("GIT_COMMITTER_EMAIL", "hotsheet@example.invalid")
            .arg(&source)
            .arg("-C")
            .arg(&store_path)
            .arg("--migrator")
            .arg(&exporter);
        command
    };
    migrate().assert().success();
    let store = FsStore::open(&store_path).unwrap().with_deferred_push();
    let row = diagnose(&store_path, &file).unwrap().remove(0);
    store
        .remove_attachment(
            &row.ticket_id,
            &row.attachment_id,
            "2026-02-01T00:00:00Z".into(),
        )
        .unwrap();
    migrate()
        .assert()
        .failure()
        .stderr(predicate::str::contains("exported HS1 attachment"));
    assert!(
        !store_path
            .join(".git/hotsheet-hs1-import-completed.json")
            .exists()
    );
    cli(&store_path)
        .arg("import")
        .arg(&file)
        .args(["--confirm-omission", &row.selection])
        .assert()
        .success();
    assert!(
        store
            .read_attachment(&row.ticket_id, &row.attachment_id)
            .is_err()
    );
    git(
        &store_path,
        &["ls-files", "--error-unmatch", OMISSIONS_FILE],
    );
    migrate().assert().success();
    let remote = root.path().join("backup.git");
    std::fs::create_dir(&remote).unwrap();
    git(&remote, &["init", "--bare", "--quiet"]);
    git(
        &store_path,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&store_path, &["push", "origin", "HEAD"]);
    Command::cargo_bin("hotsheet-migrate")
        .unwrap()
        .arg("-C")
        .arg(&store_path)
        .arg("--verify-backup")
        .assert()
        .success();
    let proof = std::fs::read(store_path.join(".git/hotsheet-hs1-backup.json")).unwrap();
    // Repeating approval preserves the proof and audit timestamp; restoring revokes it.
    cli(&store_path)
        .arg("import")
        .arg(&file)
        .args(["--confirm-omission", &row.selection])
        .assert()
        .success();
    assert_eq!(
        std::fs::read(store_path.join(".git/hotsheet-hs1-backup.json")).unwrap(),
        proof
    );
    cli(&store_path)
        .arg("import")
        .arg(&file)
        .args(["--restore-attachment", &row.selection])
        .assert()
        .success();
    assert!(!store_path.join(".git/hotsheet-hs1-backup.json").exists());
    assert_eq!(
        store
            .read_attachment(&row.ticket_id, &row.attachment_id)
            .unwrap()
            .1,
        b"FIRST"
    );
    Command::cargo_bin("hotsheet-migrate")
        .unwrap()
        .arg("-C")
        .arg(&store_path)
        .arg("--verify-backup")
        .assert()
        .failure();
    migrate().assert().success();
    git(&store_path, &["push", "origin", "HEAD"]);
    Command::cargo_bin("hotsheet-migrate")
        .unwrap()
        .arg("-C")
        .arg(&store_path)
        .arg("--verify-backup")
        .assert()
        .success();
}

#[test]
fn cli_rejects_conflicting_diagnostic_and_mutation_flags() {
    let (_root, store_path, file) = fixture();
    cli(&store_path)
        .arg("import")
        .arg(&file)
        .args(["--diagnose-attachments", "--restore-attachment", "any"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
    assert!(!store_path.exists());
}
