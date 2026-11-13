//! HS2-X4RZ73 headless E2E: `hotsheet-cli checkout remove` removes a registered checkout from
//! an isolated machine registry, idempotently, and defers to a running server when one is
//! registered so a project open in a window is refused.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};

use assert_cmd::Command;
use predicates::str::contains;
use serde_json::Value;

fn hs(home: &Path, store: &Path) -> Command {
    let mut cmd = Command::cargo_bin("hotsheet-cli").unwrap();
    cmd.env("HOTSHEET_HOME", home)
        .env_remove("HOTSHEET_STORE")
        .env_remove("HOTSHEET_ACTOR_ROLE")
        .env_remove("HOTSHEET_ACTOR_ID")
        .arg("-C")
        .arg(store);
    cmd
}

fn json(cmd: &mut Command) -> Value {
    serde_json::from_slice(&cmd.assert().success().get_output().stdout).unwrap()
}

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn home(&self) -> std::path::PathBuf {
        self.root.path().join("home")
    }
    fn store(&self) -> std::path::PathBuf {
        self.root.path().join("app.hs2")
    }
    fn run(&self, args: &[&str]) -> Command {
        let mut cmd = hs(&self.home(), &self.store());
        cmd.args(args);
        cmd
    }
    fn registry(&self) -> Vec<u8> {
        std::fs::read(self.home().join("checkouts.json")).unwrap()
    }
}

/// `app` links its Git store, `github-main` (only it), and `github-docs` (shared with `docs`).
fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let f = Fixture { root };
    for project in ["app", "docs"] {
        std::fs::create_dir_all(f.root.path().join(project)).unwrap();
    }
    hotsheet_ticketing::FsStore::init_git(f.store(), &hotsheet_ticketing::StoreMetadata::new("AP"))
        .unwrap();
    std::fs::write(
        f.store().join("providers.json"),
        r#"{"connections":[
            {"id":"github-main","provider":"github","locator":"acme/main","settings":{"credential":{"secret":"synthetic-pat"}}},
            {"id":"github-docs","provider":"github","locator":"acme/docs","settings":{"credential":{"secret":"synthetic-pat"}}}
        ]}"#,
    )
    .unwrap();
    let app = f.root.path().join("app");
    f.run(&[
        "checkout",
        "register",
        app.to_str().unwrap(),
        "--store",
        f.store().to_str().unwrap(),
    ])
    .assert()
    .success();
    let docs = f.root.path().join("docs");
    f.run(&["checkout", "register", docs.to_str().unwrap()])
        .assert()
        .success();
    for (alias, id, locator) in [
        ("app", "github-main", "acme/main"),
        ("app", "github-docs", "acme/docs"),
        ("docs", "github-docs", "acme/docs"),
    ] {
        f.run(&["checkout", "add-source", alias, id, "github", locator])
            .assert()
            .success();
    }
    f
}

#[test]
fn hs2_x4rz73_remove_is_idempotent_and_keeps_shared_sources_and_files() {
    let f = fixture();
    let before = f.registry();
    let plan = json(&mut f.run(&["checkout", "remove", "app", "--dry-run", "--json"]));
    assert_eq!(plan["registered"], true, "{plan}");
    assert_eq!(plan["removed"], false);
    assert_eq!(
        plan["removed_connections"],
        serde_json::json!(["github-main"])
    );
    assert_eq!(f.registry(), before, "a dry run writes nothing");

    let app_id = plan["checkout_id"].as_str().unwrap().to_owned();
    f.run(&["checkout", "remove", &app_id])
        .assert()
        .success()
        .stdout(contains(format!("Removed checkout app ({app_id})")))
        .stdout(contains("Kept ticket store"))
        .stdout(contains(
            "Deleted connection 'github-main': no other project uses it.",
        ))
        .stdout(contains(
            "Kept connection 'github-docs', still used by: docs-",
        ))
        .stdout(contains("Account 'synthetic-pat' stays signed in."));

    let listed = json(&mut f.run(&["checkout", "list"]));
    let aliases: Vec<_> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["alias"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(aliases, vec!["docs".to_owned()]);
    let providers = std::fs::read_to_string(f.store().join("providers.json")).unwrap();
    assert!(!providers.contains("github-main"), "{providers}");
    assert!(providers.contains("github-docs"), "{providers}");
    assert!(
        f.root.path().join("app").is_dir(),
        "the checkout directory stays"
    );
    assert!(
        f.store().join("hotsheet-store.json").is_file(),
        "the ticket store stays"
    );

    // Repeating the removal, by id or by path, succeeds and changes nothing.
    let after = f.registry();
    f.run(&["checkout", "remove", &app_id])
        .assert()
        .success()
        .stdout(contains(format!(
            "No checkout is registered as '{app_id}'; nothing to remove."
        )));
    let again = json(&mut f.run(&[
        "checkout",
        "remove",
        f.root.path().join("app").to_str().unwrap(),
        "--json",
    ]));
    assert_eq!(again["registered"], false, "{again}");
    assert_eq!(f.registry(), after);
}

#[test]
fn hs2_x4rz73_an_id_prefix_never_removes_a_checkout() {
    let f = fixture();
    let before = f.registry();
    f.run(&["checkout", "remove", "app-"])
        .assert()
        .success()
        .stdout(contains("nothing to remove"));
    assert_eq!(f.registry(), before);
}

/// A minimal HTTP responder standing in for a running server's `DELETE /checkouts/{id}`.
fn fake_server(status: u16, body: String) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            let mut secret = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("x-hotsheet-secret:") {
                    secret = value.trim().to_owned();
                }
            }
            record
                .lock()
                .unwrap()
                .push(format!("{} secret={secret}", request.trim()));
            let reason = if status == 200 { "OK" } else { "Conflict" };
            write!(
                stream,
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    (url, seen)
}

fn register_server(f: &Fixture, url: &str) {
    let instances = f.home().join("instances");
    std::fs::create_dir_all(&instances).unwrap();
    std::fs::write(
        instances.join("fixture.json"),
        serde_json::json!({
            "pid": std::process::id(), "url": url, "secret": "fixture-only",
            "store_path": f.store(), "started_at": "2026-01-01T00:00:00Z"
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn hs2_x4rz73_a_running_server_is_asked_first_and_performs_the_removal() {
    let f = fixture();
    let id = json(&mut f.run(&["checkout", "resolve", "docs"]))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let report = serde_json::json!({
        "reference": id, "registered": true, "removed": true, "checkout_id": id,
        "alias": "docs", "root": "/synthetic/docs"
    });
    let (url, seen) = fake_server(200, report.to_string());
    register_server(&f, &url);
    let before = f.registry();
    let out = json(&mut f.run(&["checkout", "remove", "docs", "--json"]));
    assert_eq!(
        out["reference"], "docs",
        "the report names the caller's reference"
    );
    assert_eq!(out["removed"], true);
    assert_eq!(
        *seen.lock().unwrap(),
        vec![
            format!("DELETE /checkouts/{id}?dry_run=true HTTP/1.1 secret=fixture-only"),
            format!("DELETE /checkouts/{id} HTTP/1.1 secret=fixture-only"),
        ]
    );
    assert_eq!(
        f.registry(),
        before,
        "the server, not the CLI, wrote the registry"
    );

    // A dry run only asks.
    seen.lock().unwrap().clear();
    f.run(&["checkout", "remove", "docs", "--dry-run"])
        .assert()
        .success();
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn hs2_x4rz73_an_open_project_or_unreachable_server_refuses_removal() {
    let f = fixture();
    let (url, seen) = fake_server(
        409,
        r#"{"error":"project docs (docs-x) is open in a Hot Sheet window; close it there, then remove it"}"#
            .into(),
    );
    register_server(&f, &url);
    let before = f.registry();
    f.run(&["checkout", "remove", "docs"])
        .assert()
        .failure()
        .stderr(contains("is open in a Hot Sheet window"));
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "only the dry-run probe was sent"
    );
    assert_eq!(f.registry(), before);

    register_server(&f, "http://127.0.0.1:1");
    f.run(&["checkout", "remove", "docs"])
        .assert()
        .failure()
        .stderr(contains("must confirm the project is closed"));
    assert_eq!(f.registry(), before);

    // An unregistered reference needs no server and still succeeds.
    f.run(&["checkout", "remove", "never-registered"])
        .assert()
        .success()
        .stdout(contains("nothing to remove"));
}

/// HS2-HM0Y6X headless E2E: `checkout prune` unregisters abandoned temporary checkouts and
/// deletes only their generated worklist skeleton. The home sits under Cargo's target
/// tmp dir (not an OS temp dir) so the prune treats it as a real machine registry.
#[test]
fn hs2_hm0y6x_prune_unregisters_abandoned_temporary_checkouts() {
    let home_root = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let home = home_root.path().join("home");
    let scratch = tempfile::tempdir().unwrap();
    let store = scratch.path().join("app.hs2");
    hotsheet_ticketing::FsStore::init_git(&store, &hotsheet_ticketing::StoreMetadata::new("AP"))
        .unwrap();
    let run = |args: &[&str]| {
        let mut cmd = hs(&home, &store);
        cmd.args(args);
        cmd
    };
    let register = |name: &str| {
        let root = scratch.path().join(name);
        std::fs::create_dir_all(&root).unwrap();
        run(&[
            "checkout",
            "register",
            root.to_str().unwrap(),
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .success();
        root
    };
    let gone = register("gone");
    std::fs::remove_dir_all(&gone).unwrap();
    let skeleton = register("skeleton");
    std::fs::create_dir_all(skeleton.join(".hotsheet2")).unwrap();
    std::fs::write(skeleton.join(".hotsheet2/worklist.md"), "# Up Next\n").unwrap();
    let live = register("live");
    std::fs::write(live.join("README.md"), "real project\n").unwrap();

    let plan = json(&mut run(&["checkout", "prune", "--dry-run", "--json"]));
    assert_eq!(plan["removed"].as_array().unwrap().len(), 2, "{plan}");
    assert!(skeleton.join(".hotsheet2/worklist.md").is_file());

    let done = json(&mut run(&["checkout", "prune", "--json"]));
    assert_eq!(done["removed"].as_array().unwrap().len(), 2, "{done}");
    assert!(
        !skeleton.exists(),
        "only the generated skeleton was there, so it is gone"
    );
    assert!(live.join("README.md").is_file());
    let left = json(&mut run(&["checkout", "list"]));
    let roots: Vec<_> = left
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["root"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(roots.len(), 1, "{roots:?}");
    assert!(roots[0].ends_with("/live"));

    run(&["checkout", "prune"])
        .assert()
        .success()
        .stdout(contains("No abandoned temporary checkouts are registered."));
}

#[test]
fn hs2_hm0y6x_prune_leaves_a_temporary_test_home_alone() {
    let f = fixture();
    let report = json(&mut f.run(&["checkout", "prune", "--json"]));
    assert!(report["skipped"].is_string(), "{report}");
    f.run(&["checkout", "prune"])
        .assert()
        .success()
        .stdout(contains("Nothing pruned"));
}
