//! HS2-FMHH9Y: the shipped serverless `hotsheet-mcp --path` process serves
//! `hotsheet_resource_claim` from the machine home. Two sessions share one
//! `HOTSHEET_HOME`, like a holder and a waiter in separate terminals. Synthetic names only.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

#[path = "support/git.rs"]
mod git_fixture;

use hotsheet_ticketing::StoreMetadata;
use serde_json::{Value, json};

const PROMPT: Duration = Duration::from_secs(30);

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Session {
    fn start(root: &Path) -> Self {
        let mut child = Command::new(workspace_bin::path("hotsheet-mcp"))
            .arg("--path")
            .arg(root.join("demo.hs2"))
            .current_dir(root)
            .env("HOTSHEET_HOME", root.join("home"))
            .env_remove("HOTSHEET_STORE")
            .env_remove("HOTSHEET_SERVER")
            .env_remove("HOTSHEET_SECRET")
            .env_remove("HOTSHEET_ACTOR_ROLE")
            .env_remove("HOTSHEET_ACTOR_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self {
            stdin: child.stdin.take().unwrap(),
            stdout: BufReader::new(child.stdout.take().unwrap()),
            child,
            next_id: 0,
        }
    }

    fn send(&mut self, arguments: Value) {
        self.next_id += 1;
        let request = json!({"jsonrpc":"2.0","id":self.next_id,"method":"tools/call",
            "params":{"name":"hotsheet_resource_claim","arguments":arguments}});
        writeln!(self.stdin, "{request}").unwrap();
    }

    /// The tool result: parsed JSON on success, or the error text.
    fn receive(&mut self) -> Result<Value, String> {
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["id"], self.next_id, "{reply}");
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        if reply["result"]["isError"] == true {
            Err(text.to_owned())
        } else {
            Ok(serde_json::from_str(text).unwrap())
        }
    }

    fn call(&mut self, arguments: Value) -> Result<Value, String> {
        self.send(arguments);
        self.receive()
    }

    fn finish(mut self) {
        drop(self.stdin);
        self.child.wait().unwrap();
    }
}

fn root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    git_fixture::init(&root.path().join("demo.hs2"), &StoreMetadata::new("HS"));
    root
}

fn until_waiting(root: &Path) {
    let waiters = root.join("home/resource-claims/namespaces/vms/waiters");
    let started = Instant::now();
    while std::fs::read_dir(&waiters).map_or(0, Iterator::count) == 0 {
        assert!(started.elapsed() < PROMPT, "waiter never registered");
        std::thread::yield_now();
    }
}

#[test]
fn hs2_fmhh9y_mcp_tool_is_listed_with_actor_arguments() {
    let root = root();
    let mut session = Session::start(root.path());
    writeln!(
        session.stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})
    )
    .unwrap();
    let mut line = String::new();
    session.stdout.read_line(&mut line).unwrap();
    let reply: Value = serde_json::from_str(&line).unwrap();
    let tool = reply["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "hotsheet_resource_claim")
        .expect("tool listed");
    let properties = &tool["inputSchema"]["properties"];
    assert_eq!(
        properties["action"]["enum"],
        json!(["acquire", "renew", "release", "show", "list"])
    );
    for name in [
        "target",
        "namespace",
        "duration",
        "wait",
        "metadata",
        "renewal_token",
        "actor_role",
    ] {
        assert!(properties[name].is_object(), "{name}");
    }
    session.finish();
}

#[test]
fn hs2_fmhh9y_mcp_acquire_conflict_token_renew_release_and_revocation() {
    let root = root();
    let mut holder = Session::start(root.path());
    let mut other = Session::start(root.path());

    let grant = holder
        .call(
            json!({"action":"acquire","target":"vms/mac-1","duration":"10m",
            "metadata":{"worker":"hmac-sha256:agent-1","note":"ui"},
            "actor_role":"ai","actor_id":"agent-1"}),
        )
        .unwrap();
    assert_eq!(grant["acquired"], true);
    assert_eq!(grant["fencing_token"], 1);
    assert_eq!(grant["metadata"]["note"], "plain:ui");
    assert!(grant["claim_id"].is_string() && grant["expires_at"].is_string());
    let token = grant["renewal_token"].as_str().unwrap().to_owned();

    // Try-now conflict from another session: an answer naming the holder, not an error.
    let refused = other
        .call(json!({"action":"acquire","target":"vms/mac-1","duration":"10m"}))
        .unwrap();
    assert_eq!(refused["acquired"], false);
    assert_eq!(refused["code"], "held_by");
    assert_eq!(refused["holder"]["claim_id"], grant["claim_id"]);
    assert_eq!(refused["holder"]["metadata"], grant["metadata"]);
    assert!(!refused.to_string().contains(&token));

    let shown = other
        .call(json!({"action":"show","target":"vms/mac-1"}))
        .unwrap();
    assert_eq!(shown["held"], true);
    let listed = other
        .call(json!({"action":"list","namespace":"vms"}))
        .unwrap();
    assert_eq!(listed["claims"].as_array().unwrap().len(), 1);
    assert!(!shown.to_string().contains(&token) && !listed.to_string().contains(&token));

    // The token alone renews from a different process that never held the claim.
    let renewed = other
        .call(json!({"action":"renew","renewal_token":token,"duration":"20m"}))
        .unwrap();
    assert_eq!(renewed["claim"]["claim_id"], grant["claim_id"]);
    assert!(renewed["claim"]["expires_at_ms"].as_u64() > grant["expires_at_ms"].as_u64());

    // Rejected for another claim, by target check and by a re-addressed secret.
    let second = other
        .call(json!({"action":"acquire","target":"vms/mac-2","duration":"10m"}))
        .unwrap();
    let second_token = second["renewal_token"].as_str().unwrap().to_owned();
    let wrong = other
        .call(json!({"action":"release","target":"vms/mac-2","renewal_token":token}))
        .unwrap_err();
    assert!(wrong.contains("token_invalid"), "{wrong}");
    let parts_one: Vec<&str> = token.split('.').collect();
    let parts_two: Vec<&str> = second_token.split('.').collect();
    let forged = [parts_two[0], parts_two[1], parts_two[2], parts_one[3]].join(".");
    let wrong = other
        .call(json!({"action":"release","renewal_token":forged}))
        .unwrap_err();
    assert!(
        wrong.contains("token_invalid") && wrong.contains("403"),
        "{wrong}"
    );
    let listed = other
        .call(json!({"action":"list","namespace":"vms"}))
        .unwrap();
    assert_eq!(listed["claims"].as_array().unwrap().len(), 2);

    // Release from the other process, then the token is revoked.
    let released = other
        .call(json!({"action":"release","renewal_token":token}))
        .unwrap();
    assert_eq!(released["released"], true);
    for arguments in [
        json!({"action":"release","renewal_token":token}),
        json!({"action":"renew","renewal_token":token,"duration":"1m"}),
    ] {
        let revoked = holder.call(arguments).unwrap_err();
        assert!(revoked.contains("token_invalid"), "{revoked}");
    }

    for (arguments, expected) in [
        (json!({}), "action is required"),
        (
            json!({"action":"steal"}),
            "unsupported resource claim action",
        ),
        (
            json!({"action":"acquire","duration":"1m"}),
            "target is required",
        ),
        (
            json!({"action":"acquire","target":"vms/mac-9"}),
            "duration is required",
        ),
        (
            json!({"action":"acquire","target":"vms/mac-9","duration":5}),
            "duration must be a string",
        ),
        (
            json!({"action":"acquire","target":"vms/mac-9","duration":"1m","metadata":[]}),
            "metadata must be an object",
        ),
        (
            json!({"action":"acquire","target":"vms/mac-9","duration":"1m","wait":"11m"}),
            "at most 10 minutes",
        ),
        (
            json!({"action":"acquire","target":"VMS/mac-9","duration":"1m"}),
            "invalid",
        ),
        (
            json!({"action":"renew","duration":"1m"}),
            "renewal_token is required",
        ),
        (
            json!({"action":"release","renewal_token":"junk"}),
            "token_invalid",
        ),
        (json!({"action":"list"}), "namespace is required"),
    ] {
        let error = holder.call(arguments.clone()).unwrap_err();
        assert!(error.contains(expected), "{arguments}: {error}");
    }
    let shown = holder
        .call(json!({"action":"show","target":"vms/mac-9"}))
        .unwrap();
    assert_eq!(shown["held"], false, "refused calls claimed nothing");
    holder.finish();
    other.finish();
}

#[test]
fn hs2_fmhh9y_mcp_wait_is_satisfied_by_release_by_expiry_or_times_out() {
    let root = root();
    let mut holder = Session::start(root.path());
    let mut waiter = Session::start(root.path());

    // Satisfied by a release from the other session.
    let held = holder
        .call(json!({"action":"acquire","target":"vms/mac-1","duration":"10m"}))
        .unwrap();
    let started = Instant::now();
    waiter.send(json!({"action":"acquire","target":"vms/mac-1","duration":"1500ms","wait":"5m"}));
    until_waiting(root.path());
    holder
        .call(json!({"action":"release","renewal_token":held["renewal_token"]}))
        .unwrap();
    let second = waiter.receive().unwrap();
    assert!(started.elapsed() < PROMPT, "woken by the release");
    assert_eq!(second["acquired"], true);
    assert_eq!(second["fencing_token"], 2);

    // Satisfied by expiry: the waiter's own 1.5 s lease lapses unrenewed.
    let third = holder
        .call(json!({"action":"acquire","target":"vms/mac-1","duration":"10m","wait":"5m"}))
        .unwrap();
    assert_eq!(third["acquired"], true);
    assert_eq!(third["fencing_token"], 3);
    assert!(third["acquired_at_ms"].as_u64() >= second["expires_at_ms"].as_u64());
    let stale = waiter
        .call(json!({"action":"renew","renewal_token":second["renewal_token"],"duration":"1m"}))
        .unwrap_err();
    assert!(stale.contains("not_holder"), "{stale}");

    // Times out naming the holder.
    let started = Instant::now();
    let refused = waiter
        .call(json!({"action":"acquire","target":"vms/mac-1","duration":"1m","wait":"800ms"}))
        .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(800));
    assert_eq!(refused["acquired"], false);
    assert_eq!(refused["code"], "wait_timeout");
    assert_eq!(refused["holder"]["claim_id"], third["claim_id"]);

    // `show` with a wait reports the holder on timeout and free after a release.
    let still = waiter
        .call(json!({"action":"show","target":"vms/mac-1","wait":"300ms"}))
        .unwrap();
    assert_eq!(still["held"], true);
    waiter.send(json!({"action":"show","target":"vms/mac-1","wait":"5m"}));
    until_waiting(root.path());
    holder
        .call(json!({"action":"release","renewal_token":third["renewal_token"]}))
        .unwrap();
    assert_eq!(waiter.receive().unwrap()["held"], false);
    holder.finish();
    waiter.finish();
}
