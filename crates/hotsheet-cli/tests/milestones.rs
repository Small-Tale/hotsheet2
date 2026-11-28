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
        serde_json::json!({"registry": true, "time_zone": "UTC", "milestones": []})
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
        serde_json::json!({"registry": false, "time_zone": "UTC", "milestones": []})
    );
}

fn created_slug(stdout: &str) -> String {
    stdout
        .strip_prefix("Created ")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("{stdout}"))
        .to_string()
}

/// The `key: value` frontmatter line of a ticket's file, as `show` prints it.
fn frontmatter(shown: &str, key: &str) -> Option<String> {
    shown
        .lines()
        .take_while(|line| !line.starts_with("<!--"))
        .find_map(|line| line.strip_prefix(&format!("{key}: ")))
        .map(|value| value.trim_matches('\'').to_string())
}

#[test]
fn hs2_a820c1_cli_links_tickets_by_title_filters_and_rolls_up() {
    let p = project();
    let alpha = created_id(&p.stdout(&["milestone", "create", "Alpha", "--due", "2026-01-15"]));
    let beta = created_id(&p.stdout(&["milestone", "create", "Beta"]));

    // `new` links by title and sets a due date; an unknown title writes nothing.
    let a = created_slug(&p.stdout(&[
        "new",
        "First",
        "--details",
        "x",
        "--milestone",
        "alpha",
        "--due",
        "2026-12-01",
    ]));
    let b = created_slug(&p.stdout(&["new", "Second", "--details", "x"]));
    let c = created_slug(&p.stdout(&["new", "Third", "--details", "x"]));
    p.refused(
        &["new", "Nope", "--milestone", "Gamma"],
        "milestone_not_found",
    );
    p.refused(&["new", "Nope", "--due", "2026-02-30"], "invalid_due_on");
    let shown = p.stdout(&["show", &a]);
    assert_eq!(
        frontmatter(&shown, "milestone"),
        Some(alpha.clone()),
        "{shown}"
    );
    assert_eq!(frontmatter(&shown, "due_on").as_deref(), Some("2026-12-01"));

    // `edit` links by id or title without changing updated_at; repeats change nothing.
    let before = frontmatter(&p.stdout(&["show", &b]), "updated_at");
    p.hs()
        .args(["edit", &b, "--milestone", &alpha])
        .assert()
        .success();
    p.hs()
        .args(["edit", &c, "--milestone", "Beta", "--due", "2027-02-01"])
        .assert()
        .success();
    let shown = p.stdout(&["show", &b]);
    assert_eq!(frontmatter(&shown, "milestone"), Some(alpha.clone()));
    assert_eq!(
        frontmatter(&shown, "updated_at"),
        before,
        "a link is not content"
    );
    p.refused(&["edit", &b, "--milestone", "Gamma"], "milestone_not_found");
    p.refused(&["edit", &b, "--due", "soon"], "invalid_due_on");

    // `ls` filters by title, id, presence and effective due.
    let ls = |args: &[&str]| {
        let mut full = vec!["ls"];
        full.extend_from_slice(args);
        let out = p.stdout(&full);
        let mut slugs: Vec<String> = out
            .lines()
            .filter_map(|line| line.split_whitespace().find(|word| word.starts_with("HS-")))
            .map(str::to_string)
            .collect();
        slugs.sort();
        slugs
    };
    let mut ab = vec![a.clone(), b.clone()];
    ab.sort();
    assert_eq!(ls(&["--milestone", "ALPHA"]), ab);
    assert_eq!(ls(&["--milestone", &beta]), vec![c.clone()]);
    assert_eq!(ls(&["--has-milestone"]).len(), 3);
    assert!(ls(&["--no-milestone"]).is_empty());
    // Alpha is due 2026-01-15 and open, so a and b are due by then; c only by its own date.
    assert_eq!(ls(&["--due-before", "2026-01-31"]), ab);
    assert_eq!(ls(&["--due-before", "2027-02-01"]).len(), 3);
    assert!(
        p.stdout(&["ls", "--milestone", "Alpha"])
            .contains("[Alpha]")
    );

    // The list carries roll-ups and the zone; show reads one by title.
    let listing = p.json(&["milestones", "--json"]);
    assert_eq!(listing["time_zone"], "UTC");
    let alpha_row = &listing["milestones"][0];
    assert_eq!(alpha_row["id"], alpha.as_str());
    assert_eq!(alpha_row["rollup"]["progress"]["members"], 2);
    assert_eq!(alpha_row["rollup"]["health"], "off_track");
    let shown = p.json(&["milestone", "show", "alpha", "--json"]);
    assert_eq!(shown["rollup"]["open_members"].as_array().unwrap().len(), 2);
    assert!(p.stdout(&["milestones"]).contains("0 of 2 finished (0%)"));

    // Closing lists the open tickets; move-open moves them; delete clears members first.
    let closed = p.stdout(&["milestone", "close", "Alpha"]);
    assert!(closed.contains("2 open tickets remain"), "{closed}");
    let moved = p.stdout(&["milestone", "move-open", "Alpha", "Beta"]);
    assert!(moved.contains("Moved 2 open tickets"), "{moved}");
    assert_eq!(ls(&["--milestone", "Beta"]).len(), 3);
    let deleted = p.stdout(&["milestone", "delete", "Beta"]);
    assert!(deleted.contains("cleared it from 3 tickets"), "{deleted}");
    assert!(!p.file(&beta).exists());
    assert_eq!(ls(&["--no-milestone"]).len(), 3);
    p.refused(&["milestone", "delete", &beta], "milestone_not_found");

    // Clearing a date and a link.
    p.hs()
        .args(["edit", &c, "--clear-due", "--clear-milestone"])
        .assert()
        .success();
    let shown = p.stdout(&["show", &c]);
    assert_eq!(frontmatter(&shown, "due_on"), None);
    assert_eq!(frontmatter(&shown, "milestone"), None);
}

#[test]
fn hs2_a820c1_cli_outside_a_checkout_takes_ids_and_lists_them_as_unknown() {
    let p = project();
    let bare = |args: &[&str]| p.cli(&p.elsewhere).args(args).assert();
    bare(&["new", "Titled", "--milestone", "Alpha"])
        .failure()
        .stderr(predicate::str::contains("milestone_registry_required"));
    let out = bare(&["new", "By id", "--milestone", "7K3M9P2QXA"]).success();
    let slug = created_slug(&String::from_utf8(out.get_output().stdout.clone()).unwrap());
    let out = bare(&["milestones", "--json"]).success();
    let listing: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(listing["registry"], false);
    assert_eq!(listing["milestones"][0]["id"], "7K3M9P2QXA");
    assert_eq!(listing["milestones"][0]["state"], "unknown");
    assert_eq!(listing["milestones"][0]["rollup"]["progress"]["members"], 1);
    let out = bare(&["milestone", "show", "7K3M9P2QXA", "--json"]).success();
    let shown: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(shown["title"], "Milestone 7K3M9P2QXA");
    bare(&["milestone", "delete", "7K3M9P2QXA"])
        .failure()
        .stderr(predicate::str::contains("milestone_registry_required"));
    // Inside the checkout the registry does not name it: still listed, never dropped.
    let listing = p.json(&["milestones", "--json"]);
    assert_eq!(listing["milestones"][0]["state"], "unknown");
    let out = p.stdout(&["ls", "--milestone", "7K3M9P2QXA"]);
    assert!(
        out.contains(&slug) && out.contains("[Milestone 7K3M9P2QXA]"),
        "{out}"
    );
}
