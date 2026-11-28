//! End-to-end milestone registry flows through the built `hotsheet-cli` binary (HS2-9PYNCN,
//! docs/38 §38.6): `milestones` and `milestone create|show|edit|close|reopen` inside a code
//! checkout that is a Git clone with an upstream, so each write's commit and push is real;
//! and the `milestone_registry_required` refusal outside any project.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};

/// A store, a bare remote, and a code checkout cloned from it and linked to the store.
struct Project {
    dir: tempfile::TempDir,
    store: PathBuf,
    root: PathBuf,
    remote: PathBuf,
    elsewhere: PathBuf,
}

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = hotsheet_ticketing::git::command()
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn project() -> Project {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let remote = dir.path().join("remote.git");
    let root = dir.path().join("project");
    let elsewhere = dir.path().join("elsewhere");
    for path in [&store, &elsewhere] {
        std::fs::create_dir_all(path).unwrap();
    }
    git(
        dir.path(),
        &["init", "-q", "--bare", "-b", "main", "remote.git"],
    );
    git(
        dir.path(),
        &["clone", "-q", remote.to_str().unwrap(), "project"],
    );
    git(&root, &["config", "user.email", "dev@example.invalid"]);
    git(&root, &["config", "user.name", "Dev"]);
    git(&root, &["checkout", "-q", "-b", "main"]);
    std::fs::write(
        root.join(".gitignore"),
        "/.hotsheet2/*\n!/.hotsheet2/groups/\n!/.hotsheet2/milestones/\n",
    )
    .unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "-q", "-m", "Seed"]);
    git(&root, &["push", "-q", "-u", "origin", "main"]);
    let project = Project {
        dir,
        store,
        root,
        remote,
        elsewhere,
    };
    project
        .cli(&project.elsewhere)
        .args(["init", "--prefix", "HS"])
        .assert()
        .success();
    std::fs::create_dir_all(project.root.join(".hotsheet2")).unwrap();
    std::fs::write(
        project.root.join(".hotsheet2/store"),
        format!("{}\n", project.store.display()),
    )
    .unwrap();
    project
}

impl Project {
    fn cli(&self, cwd: &Path) -> Command {
        let home = self.dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let mut cmd = Command::new(workspace_bin::path("hotsheet-cli"));
        cmd.arg("--test-fixture-credentials")
            .current_dir(cwd)
            .env("HOTSHEET_HOME", home)
            .env_remove("HOTSHEET_ACTOR_ROLE")
            .env_remove("HOTSHEET_ACTOR_ID")
            .env_remove("HOTSHEET_STORE")
            .env_remove("HOTSHEET_NO_AUTOCOMMIT")
            .arg("-C")
            .arg(&self.store);
        cmd
    }

    fn hs(&self) -> Command {
        self.cli(&self.root)
    }

    fn stdout(&self, args: &[&str]) -> String {
        let out = self.hs().args(args).assert().success();
        String::from_utf8(out.get_output().stdout.clone()).unwrap()
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        serde_json::from_str(&self.stdout(args)).unwrap()
    }

    fn refused(&self, args: &[&str], code: &str) {
        self.hs()
            .args(args)
            .assert()
            .failure()
            .stderr(predicate::str::contains(code));
    }

    fn subjects(&self) -> Vec<String> {
        git(&self.root, &["log", "--format=%s"])
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn remote_subjects(&self) -> Vec<String> {
        git(&self.remote, &["log", "--format=%s", "refs/heads/main"])
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn file(&self, id: &str) -> PathBuf {
        self.root.join(format!(".hotsheet2/milestones/{id}.json"))
    }
}

fn created_id(stdout: &str) -> String {
    let start = stdout.rfind('(').unwrap() + 1;
    stdout[start..start + 10].to_string()
}

#[test]
fn hs2_9pyncn_cli_create_edit_close_reopen_commit_each_write_and_skip_repeats() {
    let p = project();
    assert_eq!(
        p.json(&["milestones", "--json"]),
        serde_json::json!({"registry": true, "milestones": []})
    );
    let description = p.dir.path().join("description.md");
    std::fs::write(
        &description,
        "Ship the new build toolchain.\n\n- every package\n",
    )
    .unwrap();
    let out = p.stdout(&[
        "milestone",
        "create",
        "  Toolchain   2.0 ",
        "--due",
        "2026-12-04",
        "--description-file",
        description.to_str().unwrap(),
    ]);
    let id = created_id(&out);
    assert!(
        out.starts_with("Created milestone 'Toolchain 2.0'"),
        "{out}"
    );
    assert_eq!(
        std::fs::read_to_string(p.file(&id)).unwrap(),
        "{\n  \"title\": \"Toolchain 2.0\",\n  \"description\": \"Ship the new build toolchain.\\n\\n- every package\\n\",\n  \"due_on\": \"2026-12-04\",\n  \"state\": \"open\"\n}\n"
    );
    assert_eq!(p.subjects()[0], "Create milestone Toolchain 2.0");
    assert_eq!(p.remote_subjects()[0], "Create milestone Toolchain 2.0");

    // Title resolution ignores case and spacing; show prints the record.
    let shown = p.json(&["milestone", "show", "TOOLCHAIN 2.0", "--json"]);
    assert_eq!(shown["id"], id.as_str());
    assert_eq!(shown["state"], "open");
    assert_eq!(shown["duplicate"], false);
    let text = p.stdout(&["milestone", "show", &id]);
    assert!(
        text.contains("2026-12-04") && text.contains("- every package"),
        "{text}"
    );

    // Edit: due date, a repeat (unchanged, no commit), a clear, a rename.
    p.stdout(&["milestone", "edit", &id, "--due", "2026-12-11"]);
    let repeat = p.stdout(&["milestone", "edit", "toolchain 2.0", "--due", "2026-12-11"]);
    assert!(repeat.contains("is unchanged"), "{repeat}");
    p.stdout(&["milestone", "edit", &id, "--clear-due"]);
    p.stdout(&["milestone", "edit", &id, "--title", "Toolchain 2.1"]);
    // Close, close again (unchanged), reopen, reopen again (unchanged), close not planned.
    p.stdout(&["milestone", "close", "Toolchain 2.1"]);
    assert!(
        p.stdout(&["milestone", "close", &id])
            .contains("is unchanged")
    );
    p.stdout(&["milestone", "reopen", &id]);
    assert!(
        p.stdout(&["milestone", "reopen", &id])
            .contains("is unchanged")
    );
    p.stdout(&["milestone", "close", &id, "--reason", "not-planned"]);
    let expected = vec![
        format!("Close milestone {id} as not planned"),
        format!("Reopen milestone {id}"),
        format!("Close milestone {id}"),
        format!("Rename milestone {id} to Toolchain 2.1"),
        format!("Clear due date of milestone {id}"),
        format!("Set due date of milestone {id} to 2026-12-11"),
        "Create milestone Toolchain 2.0".to_string(),
        "Seed".to_string(),
    ];
    assert_eq!(p.subjects(), expected);
    assert_eq!(p.remote_subjects(), expected);

    // Closed milestones leave the default list and return with --all.
    let open = p.json(&["milestones", "--json"]);
    assert_eq!(open["milestones"], serde_json::json!([]));
    assert_eq!(open["registry_sync"]["state"], "synced");
    let all = p.json(&["milestones", "--json", "--all"]);
    assert_eq!(all["milestones"][0]["state"], "closed");
    assert_eq!(all["milestones"][0]["closed_reason"], "not_planned");
    assert!(all["milestones"][0]["closed_at"].is_string());
    let table = p.stdout(&["milestones", "--all"]);
    assert!(
        table.contains(&id) && table.contains("not planned"),
        "{table}"
    );
    // Only the registry file is tracked; the lock and status stay machine-local.
    assert_eq!(
        git(&p.root, &["ls-files", ".hotsheet2"]),
        format!(".hotsheet2/milestones/{id}.json")
    );
}

#[test]
fn hs2_9pyncn_cli_refusals_name_their_codes() {
    let p = project();
    let alpha = created_id(&p.stdout(&["milestone", "create", "Alpha"]));
    p.refused(&["milestone", "create", "ALPHA"], "milestone_title_taken");
    p.refused(&["milestone", "create", "   "], "invalid_milestone_title");
    p.refused(
        &["milestone", "create", "Dated", "--due", "2026-02-30"],
        "invalid_due_on",
    );
    p.refused(&["milestone", "show", "Missing"], "milestone_not_found");
    p.refused(
        &["milestone", "close", &alpha, "--reason", "done"],
        "invalid_milestone_close_reason",
    );
    let beta = created_id(&p.stdout(&["milestone", "create", "Beta"]));
    p.refused(
        &["milestone", "edit", &beta, "--title", "alpha"],
        "milestone_title_taken",
    );
    // A duplicate from another clone makes the title ambiguous; the id still works.
    std::fs::write(
        p.file("0000000001"),
        "{\n  \"title\": \"alpha\",\n  \"state\": \"open\"\n}\n",
    )
    .unwrap();
    p.refused(&["milestone", "show", "Alpha"], "milestone_ambiguous");
    let listed = p.json(&["milestones", "--json"]);
    let duplicates: Vec<bool> = listed["milestones"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|milestone| {
            milestone["title"]
                .as_str()
                .unwrap()
                .eq_ignore_ascii_case("alpha")
        })
        .map(|milestone| milestone["duplicate"].as_bool().unwrap())
        .collect();
    assert_eq!(duplicates, vec![true, true]);
    assert!(
        p.stdout(&["milestones"])
            .contains("duplicate title; use the id")
    );
    // A conflicted file is listed, flagged, and refuses writes without being touched.
    let conflicted = "{\n<<<<<<< HEAD\n  \"title\": \"Ours\",\n=======\n  \"title\": \"Theirs\",\n>>>>>>> other\n  \"state\": \"open\"\n}\n";
    std::fs::write(p.file(&beta), conflicted).unwrap();
    let shown = p.json(&["milestone", "show", &beta, "--json"]);
    assert_eq!(shown["state"], "conflicted");
    assert_eq!(
        shown["conflict"]["file"],
        format!(".hotsheet2/milestones/{beta}.json")
    );
    p.refused(
        &["milestone", "close", &beta],
        "milestone_registry_conflict",
    );
    p.refused(
        &["milestone", "edit", &beta, "--due", "2027-01-01"],
        "milestone_registry_conflict",
    );
    assert_eq!(std::fs::read_to_string(p.file(&beta)).unwrap(), conflicted);
    assert!(p.stdout(&["milestones"]).contains("CONFLICTED"));

    // Outside any project there is no registry.
    let bare = |args: &[&str]| p.cli(&p.elsewhere).args(args).assert();
    bare(&["milestone", "create", "Nowhere"])
        .failure()
        .stderr(predicate::str::contains("milestone_registry_required"));
    bare(&["milestone", "show", &alpha])
        .failure()
        .stderr(predicate::str::contains("milestone_registry_required"));
    let out = bare(&["milestones", "--json"]).success();
    let listing: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(
        listing,
        serde_json::json!({"registry": false, "milestones": []})
    );
}
