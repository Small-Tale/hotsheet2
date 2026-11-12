//! End-to-end ticket group flows through the built `hotsheet-cli` binary (HS2-PDSHNN,
//! HS2-8CDFKR, HS2-9KZSD4): the headless parity of the group field, filters, listing,
//! create, rename, ungroup, merge, and the explicit migration. Names live in the
//! project's group registry, found from the working directory; a bare store takes ids.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const ID: &str = "7K3M9P2QXA";

/// A store plus a code checkout linked to it, which owns the group registry.
struct Project {
    _dir: tempfile::TempDir,
    store: PathBuf,
    root: PathBuf,
    /// A directory outside the project, for store-only use.
    elsewhere: PathBuf,
}

fn project() -> Project {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let root = dir.path().join("project");
    let elsewhere = dir.path().join("elsewhere");
    for path in [&store, &root, &elsewhere] {
        std::fs::create_dir_all(path).unwrap();
    }
    let project = Project {
        _dir: dir,
        store,
        root,
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
        static HOME: OnceLock<PathBuf> = OnceLock::new();
        let home = HOME.get_or_init(|| {
            let path = std::env::temp_dir()
                .join(format!("hotsheet-cli-group-tests-{}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            path
        });
        let mut cmd = Command::cargo_bin("hotsheet-cli").unwrap();
        cmd.arg("--test-fixture-credentials")
            .current_dir(cwd)
            .env("HOTSHEET_HOME", home)
            .env_remove("HOTSHEET_ACTOR_ROLE")
            .env_remove("HOTSHEET_ACTOR_ID")
            .env_remove("HOTSHEET_STORE")
            .arg("-C")
            .arg(&self.store);
        cmd
    }

    /// Run inside the code checkout (names resolve through its registry).
    fn hs(&self) -> Command {
        self.cli(&self.root)
    }

    /// Run outside any project (ids only).
    fn bare(&self) -> Command {
        self.cli(&self.elsewhere)
    }

    fn stdout(&self, args: &[&str]) -> String {
        let out = self.hs().args(args).assert().success();
        String::from_utf8(out.get_output().stdout.clone()).unwrap()
    }

    fn new_ticket(&self, args: &[&str]) -> (String, PathBuf) {
        let mut all = vec!["new"];
        all.extend_from_slice(args);
        let out = self.stdout(&all);
        let slug = out.split_whitespace().nth(1).unwrap().to_string();
        let path = out
            .split_once('(')
            .and_then(|(_, rest)| rest.trim().strip_suffix(')'))
            .map(PathBuf::from)
            .unwrap();
        (slug, path)
    }

    fn groups_json(&self, cmd: &mut Command) -> serde_json::Value {
        let out = cmd.args(["groups", "--json"]).assert().success();
        serde_json::from_slice(&out.get_output().stdout).unwrap()
    }

    fn group_of(&self, slug: &str) -> Option<String> {
        self.stdout(&["show", slug])
            .lines()
            .find_map(|line| line.strip_prefix("group: ").map(str::to_string))
    }

    fn registry_name(&self, id: &str) -> Option<String> {
        let text =
            std::fs::read_to_string(self.root.join(format!(".hotsheet2/groups/{id}.json"))).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["name"].as_str().map(str::to_string)
    }

    /// Rewrite a ticket file's group line, as a pre-registry client would have left it.
    fn write_legacy(&self, path: &Path, name: &str) {
        let text = std::fs::read_to_string(path).unwrap();
        let text = text
            .lines()
            .map(|line| {
                if line.starts_with("group: ") {
                    format!("group: {name}")
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(path, text + "\n").unwrap();
    }
}

#[test]
fn hs2_9kzsd4_a_bare_store_takes_ids_and_refuses_names() {
    let p = project();
    let out = p
        .bare()
        .args(["new", "--title", "by id", "--group", ID])
        .assert()
        .success();
    let slug = String::from_utf8(out.get_output().stdout.clone())
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_string();
    assert_eq!(p.group_of(&slug).as_deref(), Some(ID));
    p.bare()
        .args(["new", "--title", "by name", "--group", "Sprint"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "group names need a project checkout",
        ));
    assert_eq!(
        p.groups_json(&mut p.bare()),
        serde_json::json!({ "registry": false, "groups": [{
            "id": ID, "name": format!("Group {ID}"), "count": 1,
            "named": false, "legacy": false, "duplicate": false,
        }] })
    );
    for args in [
        vec!["group", "rename", ID, "Named"],
        vec!["group", "create", "Named"],
        vec!["group", "migrate"],
    ] {
        p.bare()
            .args(&args)
            .assert()
            .failure()
            .stderr(predicate::str::contains("checkout"));
    }
    // Inside the project the same id is named by a rename (a placeholder until then).
    assert!(p.stdout(&["groups"]).contains(&format!("Group {ID}")));
    assert!(
        p.stdout(&["group", "rename", ID, "Shared work"])
            .contains(&format!("Renamed group {ID} to 'Shared work'."))
    );
    assert_eq!(p.registry_name(ID).as_deref(), Some("Shared work"));
    assert_eq!(p.group_of(&slug).as_deref(), Some(ID), "no ticket change");
    assert!(p.stdout(&["ls"]).contains("{Shared work}"));
}

#[test]
fn hs2_9kzsd4_group_field_filters_and_listing_resolve_names_in_the_project() {
    let p = project();
    assert!(p.stdout(&["groups"]).contains("(no groups)"));
    assert_eq!(
        p.groups_json(&mut p.hs()),
        serde_json::json!({ "registry": true, "groups": [] })
    );

    let (first, _) = p.new_ticket(&["--title", "first", "--group", "  Board   Drag & Drop "]);
    let board = p.group_of(&first).unwrap();
    assert!(board.len() == 10 && board.chars().all(|c| c.is_ascii_alphanumeric()));
    assert_eq!(
        p.registry_name(&board).as_deref(),
        Some("Board Drag & Drop")
    );
    let (second, _) = p.new_ticket(&["--title", "second"]);
    let (third, _) = p.new_ticket(&["--title", "third", "--group", "alpha"]);
    let alpha = p.group_of(&third).unwrap();
    assert_eq!(p.group_of(&second), None);

    // Another spelling of a name is the same group id.
    p.hs()
        .args(["edit", &second, "--group", "BOARD drag & drop"])
        .assert()
        .success();
    assert_eq!(p.group_of(&second), Some(board.clone()));
    let summary = |id: &str, name: &str, count: usize| {
        serde_json::json!({ "id": id, "name": name, "count": count,
                            "named": true, "legacy": false, "duplicate": false })
    };
    assert_eq!(
        p.groups_json(&mut p.hs()),
        serde_json::json!({ "registry": true, "groups": [
            summary(&alpha, "alpha", 1),
            summary(&board, "Board Drag & Drop", 2),
        ] })
    );
    let table = p.stdout(&["groups"]);
    assert!(
        table.contains(&format!("    2  {board}  Board Drag & Drop")),
        "{table}"
    );
    assert!(table.find("alpha").unwrap() < table.find("Board").unwrap());

    let grouped = p.stdout(&["ls", "--group", "board   DRAG & drop"]);
    assert!(grouped.contains(&first) && grouped.contains(&second) && !grouped.contains(&third));
    assert!(grouped.contains("{Board Drag & Drop}"));
    assert_eq!(p.stdout(&["ls", "--group", &board]), grouped);
    let any = p.stdout(&["ls", "--has-group"]);
    assert!(any.contains(&first) && any.contains(&third));
    p.hs()
        .args(["edit", &second, "--clear-group"])
        .assert()
        .success();
    let none = p.stdout(&["ls", "--no-group"]);
    assert!(none.contains(&second) && !none.contains(&first) && !none.contains(&third));
    assert!(
        p.stdout(&["ls", "--group", "no such group"])
            .contains("(no tickets)")
    );

    // Conflicting flags and invalid names are refused without a write.
    p.hs()
        .args(["edit", &first, "--group", "x", "--clear-group"])
        .assert()
        .failure();
    p.hs()
        .args(["ls", "--has-group", "--no-group"])
        .assert()
        .failure();
    p.hs()
        .args(["edit", &first, "--group", "line\nbreak"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("control characters"));
    p.hs()
        .args(["new", "--title", "bad", "--group", &"x".repeat(121)])
        .assert()
        .failure()
        .stderr(predicate::str::contains("120 characters"));
    assert_eq!(p.group_of(&first), Some(board.clone()));

    // A batch resolves one name to one id for every selected ticket.
    p.hs()
        .args(["batch", &first, &second, "--group", "Release"])
        .assert()
        .success();
    let release = p.group_of(&first).unwrap();
    assert_eq!(p.group_of(&second), Some(release.clone()));
    assert_eq!(p.registry_name(&release).as_deref(), Some("Release"));
    // A trashed ticket no longer keeps its group listed.
    p.hs()
        .args(["edit", &third, "--status", "deleted"])
        .assert()
        .success();
    assert_eq!(
        p.groups_json(&mut p.hs())["groups"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn hs2_9kzsd4_create_rename_ungroup_and_merge_work_headlessly() {
    let p = project();
    let created = p.stdout(&["group", "create", "Old"]);
    assert!(created.starts_with("Created group 'Old' ("), "{created}");
    assert!(
        p.stdout(&["group", "create", "OLD"])
            .contains("already exists")
    );
    let (a, _) = p.new_ticket(&["--title", "a", "--group", "Old"]);
    let (b, _) = p.new_ticket(&["--title", "b", "--group", "old"]);
    let (other, _) = p.new_ticket(&["--title", "other", "--group", "Taken"]);
    let old = p.group_of(&a).unwrap();
    assert!(created.contains(&old));

    p.hs()
        .args(["group", "rename", "missing", "New"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no group named 'missing'"));
    p.hs()
        .args(["group", "rename", "Old", "taken"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "a group named 'Taken' already exists",
        ));
    p.hs()
        .args(["group", "rename", "Old", "   "])
        .assert()
        .failure();
    let before = p.stdout(&["show", &a]);
    assert!(
        p.stdout(&["group", "rename", "OLD", "New   Name"])
            .contains(&format!("Renamed group {old} to 'New Name'."))
    );
    assert_eq!(
        p.stdout(&["show", &a]),
        before,
        "a rename rewrites no ticket"
    );
    assert_eq!(p.registry_name(&old).as_deref(), Some("New Name"));
    assert!(
        p.stdout(&["group", "rename", "new name", "NEW NAME"])
            .contains("'NEW NAME'")
    );

    // Two clones created the same name: listed twice, flagged, then merged.
    std::fs::write(
        p.root.join(".hotsheet2/groups/0000000001.json"),
        "{\"name\":\"new name\"}",
    )
    .unwrap();
    p.hs()
        .args(["edit", &other, "--group", "0000000001"])
        .assert()
        .success();
    let table = p.stdout(&["groups"]);
    assert_eq!(table.matches("duplicate name").count(), 2, "{table}");
    p.hs()
        .args(["group", "ungroup", "new name"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("use a group id"));
    assert!(
        p.stdout(&["group", "merge", "0000000001", &old])
            .contains(&format!("Merged 1 ticket(s) into 'NEW NAME' ({old})."))
    );
    assert_eq!(p.group_of(&other), Some(old.clone()));
    assert!(!p.stdout(&["groups"]).contains("duplicate"));

    assert!(
        p.stdout(&["group", "ungroup", "new name"])
            .contains("Ungrouped 3 ticket(s).")
    );
    for slug in [&a, &b, &other] {
        assert_eq!(p.group_of(slug), None);
    }
    p.hs()
        .args(["group", "ungroup", "new name"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no group named"));
    assert!(
        p.registry_name(&old).is_some(),
        "the registry entry is kept"
    );
}

#[test]
fn hs2_9kzsd4_migrate_rewrites_legacy_names_once() {
    let p = project();
    assert!(
        p.stdout(&["group", "migrate"])
            .contains("No legacy group names")
    );
    let (a, a_path) = p.new_ticket(&["--title", "a", "--group", "seed"]);
    let (b, b_path) = p.new_ticket(&["--title", "b", "--group", "seed"]);
    let (c, c_path) = p.new_ticket(&["--title", "c", "--group", "seed"]);
    let (modern, _) = p.new_ticket(&["--title", "modern", "--group", "Modern"]);
    let modern_id = p.group_of(&modern).unwrap();
    p.write_legacy(&a_path, "Board Drag");
    p.write_legacy(&b_path, "board drag");
    p.write_legacy(&c_path, "Search");
    // Legacy names are read and listed by name, flagged for migration.
    let table = p.stdout(&["groups"]);
    assert!(table.contains("Board Drag  (legacy name"), "{table}");
    assert!(p.stdout(&["ls", "--group", "BOARD DRAG"]).contains(&a));

    let out = p.stdout(&["group", "migrate"]);
    assert!(
        out.contains("Migrated 3 ticket(s): created 2 group(s), reused 0 existing group(s)."),
        "{out}"
    );
    let board = p.group_of(&a).unwrap();
    assert_eq!(p.group_of(&b), Some(board.clone()));
    assert_eq!(p.registry_name(&board).as_deref(), Some("Board Drag"));
    let search = p.group_of(&c).unwrap();
    assert_eq!(p.registry_name(&search).as_deref(), Some("Search"));
    assert_eq!(p.group_of(&modern), Some(modern_id));
    assert!(!p.stdout(&["groups"]).contains("legacy"));
    // Rerun changes nothing.
    assert!(
        p.stdout(&["group", "migrate"])
            .contains("No legacy group names")
    );
    assert_eq!(p.group_of(&a), Some(board));
}
