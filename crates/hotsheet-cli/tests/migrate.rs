//! E2E for the standalone `hotsheet-migrate` binary. The happy path (spawning the
//! Node exporter) is exercised by the migrator's vitest suite; here we cover the
//! arg surface + the missing-migrator error offline.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use assert_cmd::Command;
use predicates::prelude::*;

fn fake_exporter(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("export.mjs");
    std::fs::write(
        &path,
        r#"import {writeFileSync} from 'node:fs';
const args=process.argv.slice(2),out=args[args.indexOf('--out')+1];
writeFileSync(out,JSON.stringify({exportVersion:1,project:{name:'Demo',ticketPrefix:'HS'},tickets:[{ticket_number:'HS-1',title:'Migrated once',status:'not_started',created_at:'2026-01-01T00:00:00Z',updated_at:'2026-01-01T00:00:00Z'}]}));
"#,
    )
    .unwrap();
    path
}

fn migration_command(
    source: &std::path::Path,
    store: &std::path::Path,
    exporter: &std::path::Path,
) -> Command {
    let mut command = Command::new(workspace_bin::path("hotsheet-migrate"));
    command
        .env("GIT_AUTHOR_NAME", "Hot Sheet test")
        .env("GIT_AUTHOR_EMAIL", "hotsheet@example.invalid")
        .env("GIT_COMMITTER_NAME", "Hot Sheet test")
        .env("GIT_COMMITTER_EMAIL", "hotsheet@example.invalid")
        .args([
            source.to_str().unwrap(),
            "-C",
            store.to_str().unwrap(),
            "--migrator",
            exporter.to_str().unwrap(),
        ]);
    command
}

#[test]
fn reports_a_missing_migrator() {
    let dir = tempfile::tempdir().unwrap();
    Command::new(workspace_bin::path("hotsheet-migrate"))
        .args([
            "/tmp/nonexistent-hotsheet",
            "-C",
            dir.path().to_str().unwrap(),
            "--migrator",
            "/tmp/nope/export.mjs",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migrator not found"));
}

#[test]
fn has_its_own_help() {
    Command::new(workspace_bin::path("hotsheet-migrate"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Migrate a Hot Sheet 1 project"));
}

#[test]
fn repeated_migration_treats_a_clean_commit_as_success_without_warning() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("project/.hotsheet");
    let store = root.path().join("tickets.hs2");
    std::fs::create_dir_all(&source).unwrap();
    let exporter = fake_exporter(root.path());
    let run = || migration_command(&source, &store, &exporter);

    run().assert().success().stdout(predicate::str::contains(
        "Imported 1 ticket(s) (0 attachment file(s)), skipped 0 already present.",
    ));
    let commits_before = hotsheet_ticketing::git::command()
        .current_dir(&store)
        .args(["rev-list", "--count", "HEAD"])
        .output()
        .unwrap()
        .stdout;

    run()
        .assert()
        .success()
        .stdout(predicate::str::contains("skipped 1 already present"))
        .stderr(predicate::str::contains("warning: git commit").not());
    let commits_after = hotsheet_ticketing::git::command()
        .current_dir(&store)
        .args(["rev-list", "--count", "HEAD"])
        .output()
        .unwrap()
        .stdout;
    assert_eq!(
        commits_after, commits_before,
        "a clean re-import adds no commit"
    );
}

#[test]
fn legacy_partial_import_cannot_gain_completion_proof_from_an_existing_ticket_alone() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("project/.hotsheet");
    let store = root.path().join("tickets.hs2");
    std::fs::create_dir_all(&source).unwrap();
    let exporter = fake_exporter(root.path());
    migration_command(&source, &store, &exporter)
        .assert()
        .success();
    let content = std::fs::read_to_string(&exporter).unwrap().replace(
        "title:'Migrated once',",
        "title:'Migrated once',attachments:[{original_filename:'shot.png',stored_path:'attachments/shot.png'}],",
    );
    std::fs::write(
        &exporter,
        format!(
            "{content}\nconst {{mkdirSync}}=await import('node:fs');const {{dirname,join}}=await import('node:path');\nmkdirSync(join(dirname(out),'attachments'),{{recursive:true}});writeFileSync(join(dirname(out),'attachments','shot.png'),'payload');\n"
        ),
    )
    .unwrap();
    migration_command(&source, &store, &exporter)
        .assert()
        .failure()
        .stderr(predicate::str::contains("exported HS1 attachment"));
    assert!(
        !store
            .join(".git/hotsheet-hs1-import-completed.json")
            .exists()
    );
    assert!(source.is_dir());
    assert!(
        hotsheet_ticketing::FsStore::open(&store)
            .unwrap()
            .list_tickets()
            .unwrap()[0]
            .attachments
            .is_empty()
    );
}

#[test]
fn verifies_manually_pushed_import_and_rejects_old_remote_or_changed_origin() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("project/.hotsheet");
    let store = root.path().join("tickets.hs2");
    let remote = root.path().join("backup.git");
    std::fs::create_dir_all(&source).unwrap();
    let exporter = fake_exporter(root.path());
    migration_command(&source, &store, &exporter)
        .assert()
        .success();
    let git = |path: &std::path::Path, args: &[&str]| {
        let output = hotsheet_ticketing::git::command()
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    git(root.path(), &["init", "--bare", remote.to_str().unwrap()]);
    git(
        &store,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    let verify = || {
        let mut command = Command::new(workspace_bin::path("hotsheet-migrate"));
        command.args(["-C", store.to_str().unwrap(), "--verify-backup"]);
        command
    };
    verify().assert().failure();
    let backup = store.join(".git/hotsheet-hs1-backup.json");
    assert!(!backup.exists());
    git(&store, &["push", "-u", "origin", "HEAD"]);
    verify()
        .assert()
        .success()
        .stdout(predicate::str::contains("Verified Hot Sheet 1 backup"));
    let proof: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&backup).unwrap()).unwrap();
    assert_eq!(
        proof["revision"],
        git(&store, &["rev-parse", "HEAD"]).trim()
    );
    assert_eq!(proof["remote"], remote.to_str().unwrap());
    let branch = git(&store, &["symbolic-ref", "HEAD"]);
    let before_receipt = git(&store, &["rev-parse", "HEAD~1"]);
    git(
        &remote,
        &["update-ref", branch.trim(), before_receipt.trim()],
    );
    verify().assert().failure();
    git(
        &store,
        &[
            "remote",
            "set-url",
            "origin",
            root.path().join("missing.git").to_str().unwrap(),
        ],
    );
    verify().assert().failure();
    assert!(source.is_dir());
}

fn git_in(path: &std::path::Path, args: &[&str]) -> String {
    let output = hotsheet_ticketing::git::command()
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// HS2-HSHT2G: a migrated store already ignores the runtime directories a server fills
/// later. One that an older build left with an uncommitted ignore line is committed by
/// the backup preparation itself, a stat-only rewrite is not uncommitted work, and a
/// real uncommitted file is named.
#[test]
fn hs2_hsht2g_backup_preparation_commits_hot_sheet_ignore_rules_and_names_real_changes() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("project/.hotsheet");
    let store = root.path().join("tickets.hs2");
    std::fs::create_dir_all(&source).unwrap();
    let exporter = fake_exporter(root.path());
    migration_command(&source, &store, &exporter)
        .env("HOTSHEET_HOME", &home)
        .assert()
        .success();
    let ignore = store.join(".gitignore");
    let committed = std::fs::read_to_string(&ignore).unwrap();
    for runtime in [
        "metrics/local/",
        "metrics/raw/",
        "activity/recent/",
        "local/",
    ] {
        assert!(committed.lines().any(|line| line == runtime), "{committed}");
    }
    let status = || {
        git_in(
            &store,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )
    };
    assert_eq!(status(), "");
    let prepare = || {
        let mut command = Command::new(workspace_bin::path("hotsheet-migrate"));
        command.env("HOTSHEET_HOME", &home).args([
            "-C",
            store.to_str().unwrap(),
            "--prepare-backup",
        ]);
        command
    };

    // What an older build left behind: the committed file lacks a runtime line that a
    // server then appended.
    let legacy: String = committed
        .lines()
        .filter(|line| *line != "metrics/local/")
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(&ignore, &legacy).unwrap();
    git_in(&store, &["commit", "-qam", "older build"]);
    std::fs::write(&ignore, format!("{legacy}metrics/local/\n")).unwrap();
    assert_eq!(status(), " M .gitignore\n");
    prepare()
        .assert()
        .success()
        .stdout(predicate::str::contains("ready to back up"));
    assert_eq!(status(), "");
    assert_eq!(
        git_in(&store, &["log", "-1", "--format=%s"]).trim(),
        "Update Hot Sheet store ignore rules"
    );

    // A stat-only change (same bytes, new modification time) is not uncommitted work.
    let head = git_in(&store, &["rev-parse", "HEAD"]);
    std::fs::write(&ignore, std::fs::read(&ignore).unwrap()).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(120);
    std::fs::File::options()
        .write(true)
        .open(&ignore)
        .unwrap()
        .set_modified(later)
        .unwrap();
    prepare().assert().success();
    assert_eq!(git_in(&store, &["rev-parse", "HEAD"]), head);

    // Real uncommitted work is named rather than described generically.
    std::fs::write(store.join("notes.txt"), "user edit").unwrap();
    prepare()
        .assert()
        .failure()
        .stderr(predicate::str::contains("(notes.txt)"));
    assert_eq!(git_in(&store, &["rev-parse", "HEAD"]), head);
}

/// HS2-8SEWHJ: the headless import saves the backup remote it is given as `origin`,
/// keeps it when the push fails, replaces only an origin Hot Sheet saved, refuses a
/// user's own origin, and is idempotent on rerun.
#[test]
fn hs2_8sewhj_headless_backup_remote_is_saved_pushed_and_verified() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("project/.hotsheet");
    let store = root.path().join("tickets.hs2");
    let missing = root.path().join("missing.git");
    let remote = root.path().join("backup.git");
    std::fs::create_dir_all(&source).unwrap();
    let exporter = fake_exporter(root.path());
    git_in(
        root.path(),
        &["init", "-q", "--bare", remote.to_str().unwrap()],
    );
    let origin = || {
        git_in(&store, &["remote", "get-url", "origin"])
            .trim()
            .to_owned()
    };
    let backup = |url: &std::path::Path| {
        let mut command = Command::new(workspace_bin::path("hotsheet-migrate"));
        command
            .env("HOTSHEET_HOME", &home)
            .env("GIT_AUTHOR_NAME", "Hot Sheet test")
            .env("GIT_AUTHOR_EMAIL", "hotsheet@example.invalid")
            .env("GIT_COMMITTER_NAME", "Hot Sheet test")
            .env("GIT_COMMITTER_EMAIL", "hotsheet@example.invalid")
            .args([
                "-C",
                store.to_str().unwrap(),
                "--backup-remote",
                url.to_str().unwrap(),
            ]);
        command
    };

    // Import and back up in one headless command; the push fails, the URL stays saved.
    migration_command(&source, &store, &exporter)
        .env("HOTSHEET_HOME", &home)
        .args(["--backup-remote", missing.to_str().unwrap()])
        .assert()
        .failure()
        .stdout(predicate::str::contains("Imported 1 ticket(s)"))
        .stderr(predicate::str::contains("origin is saved as"));
    assert_eq!(origin(), missing.to_str().unwrap());
    let proof = store.join(".git/hotsheet-hs1-backup.json");
    assert!(!proof.exists());

    // A corrected URL replaces the origin Hot Sheet saved, pushes, and verifies.
    backup(&remote)
        .assert()
        .success()
        .stdout(predicate::str::contains("Saved"));
    assert_eq!(origin(), remote.to_str().unwrap());
    assert!(git_in(&store, &["remote", "-v"]).contains(remote.to_str().unwrap()));
    assert_eq!(
        git_in(&store, &["rev-parse", "HEAD"]),
        git_in(&remote, &["rev-parse", "HEAD"])
    );
    assert!(proof.is_file());
    // Rerunning is idempotent.
    backup(&remote).assert().success();
    assert_eq!(origin(), remote.to_str().unwrap());

    // An origin the user configured is never replaced.
    git_in(&store, &["config", "--unset", "hotsheet.backupOrigin"]);
    backup(&missing)
        .assert()
        .failure()
        .stderr(predicate::str::contains("refusing to replace"));
    assert_eq!(origin(), remote.to_str().unwrap());
}

#[cfg(unix)]
#[test]
fn repeated_migration_rejects_failed_commits_and_invalidates_cleanup_proof() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("project/.hotsheet");
    let store = root.path().join("tickets.hs2");
    std::fs::create_dir_all(&source).unwrap();
    let exporter = fake_exporter(root.path());
    migration_command(&source, &store, &exporter)
        .assert()
        .success();
    let proof = store.join(".git/hotsheet-hs1-import-completed.json");
    assert!(proof.is_file());

    let hook = store.join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    let mut permissions = std::fs::metadata(&hook).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&hook, permissions).unwrap();
    let changed = std::fs::read_to_string(&exporter)
        .unwrap()
        .replace("HS-1", "HS-2")
        .replace("Migrated once", "Commit must fail");
    std::fs::write(&exporter, changed).unwrap();

    migration_command(&source, &store, &exporter)
        .assert()
        .failure()
        .stderr(predicate::str::contains("warning: git commit"))
        .stderr(predicate::str::contains("not fully committed"));
    assert!(!proof.exists());
    assert!(source.is_dir());
    std::fs::remove_file(hook).unwrap();
    migration_command(&source, &store, &exporter)
        .assert()
        .success();
    assert!(proof.is_file());
}

#[cfg(unix)]
#[test]
fn machine_progress_keeps_hook_output_on_stderr_and_returns_typed_results() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("project/.hotsheet");
    let store = root.path().join("tickets.hs2");
    std::fs::create_dir_all(&source).unwrap();
    let exporter = fake_exporter(root.path());
    migration_command(&source, &store, &exporter)
        .assert()
        .success();
    let hook = store.join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\necho HUMAN-HOOK-OUTPUT\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let content = std::fs::read_to_string(&exporter)
        .unwrap()
        .replace("HS-1", "HS-2");
    std::fs::write(&exporter, content).unwrap();
    let output = migration_command(&source, &store, &exporter)
        .arg("--progress-json")
        .assert()
        .success()
        .get_output()
        .clone();
    let records: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(records.iter().all(|record| record["version"] == 1));
    assert!(
        records
            .iter()
            .any(|record| record["phase"] == "import_tickets" && record["completed"] == 1)
    );
    assert_eq!(records.last().unwrap()["result"]["tickets"], 1);
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("HUMAN-HOOK-OUTPUT")
    );
}
