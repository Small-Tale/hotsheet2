//! E2E for the standalone `hotsheet-migrate` binary. The happy path (spawning the
//! Node exporter) is exercised by the migrator's vitest suite; here we cover the
//! arg surface + the missing-migrator error offline.

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
    let mut command = Command::cargo_bin("hotsheet-migrate").unwrap();
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
    Command::cargo_bin("hotsheet-migrate")
        .unwrap()
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
    Command::cargo_bin("hotsheet-migrate")
        .unwrap()
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
    let commits_before = std::process::Command::new("git")
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
    let commits_after = std::process::Command::new("git")
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

#[cfg(unix)]
#[test]
fn repeated_migration_still_warns_when_a_real_commit_fails() {
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
        .success()
        .stdout(predicate::str::contains("Imported 1 ticket(s)"))
        .stderr(predicate::str::contains("warning: git commit"));
}
