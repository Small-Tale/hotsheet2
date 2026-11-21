//! HS2-ME31D0: a server whose owner holds its stdin (`--exit-on-stdin-eof`, as every browser and
//! test fixture starts it) still answers requests that run `git`.
//!
//! On Windows the EOF watcher keeps a synchronous read pending on the owner's stdin pipe. Each
//! `git` the server spawned inherited that pipe and blocked in its C runtime start-up until the
//! owner exited, so Code Review never left "Finding ticket commits…" and repository status
//! never answered. The request below runs `git` for the commit count and history; it must
//! answer well inside the timeout on every platform.

use std::io::{BufRead as _, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::Value;

#[path = "../../test-support/first_launch.rs"]
mod first_launch;
#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

const SECRET: &str = "hs2-me31d0-synthetic";

/// Kills the server when the test ends, even on a failed assertion.
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn git(root: &Path, args: &[&str]) {
    let status = hotsheet_ticketing::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Synthetic Test")
        .env("GIT_AUTHOR_EMAIL", "test@invalid.example")
        .env("GIT_COMMITTER_NAME", "Synthetic Test")
        .env("GIT_COMMITTER_EMAIL", "test@invalid.example")
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

#[test]
fn hs2_me31d0_git_requests_answer_while_the_owner_holds_stdin() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("tickets");
    std::fs::create_dir(&store).unwrap();
    git(&store, &["init", "--quiet", "--initial-branch=main"]);
    FsStore::init(&store, &StoreMetadata::new("TST")).unwrap();
    let root = fixture.path().join("project");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "--quiet", "--initial-branch=main"]);
    git(
        &root,
        &["commit", "--quiet", "--allow-empty", "-m", "Synthetic seed"],
    );
    let home = fixture.path().join("home");
    std::fs::create_dir(&home).unwrap();

    let log = fixture.path().join("server.log");
    let binary = first_launch::launched(workspace_bin::path("hotsheet-server"), &["--version"]);
    let mut server = Server(
        Command::new(binary)
            .arg("-C")
            .arg(&store)
            .args(["--bind", "127.0.0.1:0", "--secret", SECRET])
            .args(["--no-terminal-broker", "--exit-on-stdin-eof"])
            .env("HOTSHEET_HOME", &home)
            // Held open for the whole test: the watcher's read stays pending.
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(&log).unwrap())
            .spawn()
            .unwrap(),
    );
    let stdout = server.0.stdout.take().unwrap();
    let (found, url) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if let Some(rest) = line.split("listening on ").nth(1) {
                let _ = found.send(rest.split_whitespace().next().unwrap_or("").to_owned());
            }
        }
    });
    let url = url
        .recv_timeout(Duration::from_secs(60))
        .unwrap_or_else(|_| {
            let output = std::fs::read_to_string(&log).unwrap_or_default();
            panic!("server did not start: {output}")
        });

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    let body = serde_json::json!({ "root": root, "stores": [store] }).to_string();
    let checkout: Value = serde_json::from_str(
        &agent
            .post(&format!("{url}/checkouts"))
            .set("x-hotsheet-secret", SECRET)
            .set("content-type", "application/json")
            .send_string(&body)
            .unwrap()
            .into_string()
            .unwrap(),
    )
    .unwrap();
    let id = checkout["id"].as_str().unwrap();

    let status: Value = serde_json::from_str(
        &agent
            .get(&format!("{url}/checkouts/{id}/repository/status"))
            .set("x-hotsheet-secret", SECRET)
            .call()
            .expect("repository status answers while the owner holds stdin")
            .into_string()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(status["initialized"], true, "{status}");
    assert_eq!(status["commit_count"], 1, "{status}");
    assert!(
        server.0.try_wait().unwrap().is_none(),
        "the server keeps running"
    );
}
