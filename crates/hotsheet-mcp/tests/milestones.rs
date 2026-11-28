//! Milestones over MCP (HS2-9PYNCN, docs/38 §38.6): `hotsheet_milestones` list, show,
//! create, edit, close, and reopen, serverless against a real checkout registry in a Git
//! clone with an upstream (each write commits and pushes as it returns), and forwarded to a
//! server (the routes it must call). Without a checkout the tool refuses with
//! `milestone_registry_required` before any request.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use hotsheet_mcp::{Backend, BackendError, CoreBackend, handle_message};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::{Value, json};

fn call(backend: &dyn Backend, name: &str, mut args: Value) -> Value {
    let object = args.as_object_mut().expect("tool arguments are an object");
    object.entry("actor_role").or_insert_with(|| json!("human"));
    object
        .entry("actor_id")
        .or_insert_with(|| json!("mcp-test"));
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": name, "arguments": args }
    });
    let reply = handle_message(&request, backend).unwrap();
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    if reply["result"]["isError"] == Value::Bool(true) {
        return json!({ "error": text });
    }
    serde_json::from_str(text).unwrap_or_else(|_| json!({ "raw": text }))
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

/// A checkout cloned from a bare remote, registered with one linked store.
struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
    remote: PathBuf,
    backend: CoreBackend,
}

fn project() -> Project {
    let dir = tempfile::tempdir().unwrap();
    let remote = dir.path().join("remote.git");
    git(
        dir.path(),
        &["init", "-q", "--bare", "-b", "main", "remote.git"],
    );
    git(
        dir.path(),
        &["clone", "-q", remote.to_str().unwrap(), "project"],
    );
    let root = dir.path().join("project");
    git(&root, &["config", "user.email", "dev@example.invalid"]);
    git(&root, &["config", "user.name", "Dev"]);
    git(&root, &["checkout", "-q", "-b", "main"]);
    std::fs::write(
        root.join(".gitignore"),
        "/.hotsheet2/*\n!/.hotsheet2/milestones/\n",
    )
    .unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "-q", "-m", "Seed"]);
    git(&root, &["push", "-q", "-u", "origin", "main"]);
    let store_root = dir.path().join("AA.hs2");
    FsStore::init_git(&store_root, &StoreMetadata::new("AA")).unwrap();
    let registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(
        dir.path().join("home/checkouts.json"),
    );
    registry
        .register(&root, Some("project"), None, vec![store_root.clone()])
        .unwrap();
    let backend =
        CoreBackend::new(FsStore::open(&store_root).unwrap()).with_checkout_registry(registry);
    Project {
        _dir: dir,
        root,
        remote,
        backend,
    }
}

impl Project {
    fn tool(&self, args: Value) -> Value {
        let mut args = args;
        args["checkout"] = json!("project");
        call(&self.backend, "hotsheet_milestones", args)
    }

    fn refused(&self, args: Value, code: &str) {
        let answer = self.tool(args);
        let error = answer["error"]
            .as_str()
            .unwrap_or_else(|| panic!("{answer}"));
        assert!(error.contains(code), "{error}");
    }

    fn remote_subjects(&self) -> Vec<String> {
        git(&self.remote, &["log", "--format=%s", "refs/heads/main"])
            .lines()
            .map(str::to_string)
            .collect()
    }
}

#[test]
fn hs2_9pyncn_serverless_milestones_round_trip_through_the_checkout_registry() {
    let p = project();
    assert_eq!(
        p.tool(json!({})),
        json!({ "registry": true, "milestones": [] })
    );
    let created = p.tool(json!({
        "action": "create",
        "title": " Toolchain  2.0 ",
        "due_on": "2026-12-04",
        "description": "Ship it."
    }));
    assert_eq!(created["changed"], true, "{created}");
    let milestone = &created["milestone"];
    let id = milestone["id"].as_str().unwrap().to_string();
    assert_eq!(milestone["title"], "Toolchain 2.0");
    assert_eq!(milestone["state"], "open");
    assert_eq!(milestone["due_on"], "2026-12-04");
    assert_eq!(
        std::fs::read_to_string(p.root.join(format!(".hotsheet2/milestones/{id}.json"))).unwrap(),
        "{\n  \"title\": \"Toolchain 2.0\",\n  \"description\": \"Ship it.\",\n  \"due_on\": \"2026-12-04\",\n  \"state\": \"open\"\n}\n"
    );
    // Show by id and by a title with other case and spacing (encoded in the route).
    for name in [id.as_str(), "toolchain   2.0"] {
        let shown = p.tool(json!({ "action": "show", "milestone": name }));
        assert_eq!(shown["id"], id.as_str(), "{name}");
    }
    // Edit, repeat, clear with null; close, repeat; reopen; close as not planned.
    let edited = p.tool(json!({ "action": "edit", "milestone": id, "due_on": "2026-12-11" }));
    assert_eq!(edited["changed"], true);
    let repeat = p.tool(json!({ "action": "edit", "milestone": id, "due_on": "2026-12-11" }));
    assert_eq!(repeat["changed"], false);
    let cleared = p.tool(json!({
        "action": "edit", "milestone": id, "due_on": null, "description": null
    }));
    assert_eq!(cleared["changed"], true);
    assert!(cleared["milestone"].get("due_on").is_none());
    assert!(cleared["milestone"].get("description").is_none());
    let closed = p.tool(json!({ "action": "close", "milestone": "Toolchain 2.0" }));
    assert_eq!(closed["milestone"]["closed_reason"], "completed");
    assert_eq!(
        p.tool(json!({ "action": "close", "milestone": id }))["changed"],
        false
    );
    assert_eq!(
        p.tool(json!({ "action": "reopen", "milestone": id }))["milestone"]["state"],
        "open"
    );
    let dropped = p.tool(json!({ "action": "close", "milestone": id, "reason": "not_planned" }));
    assert_eq!(dropped["milestone"]["closed_reason"], "not_planned");
    // Every change was committed and pushed as it returned; repeats added nothing.
    assert_eq!(
        p.remote_subjects(),
        vec![
            format!("Close milestone {id} as not planned"),
            format!("Reopen milestone {id}"),
            format!("Close milestone {id}"),
            format!("Edit milestone {id}"),
            format!("Set due date of milestone {id} to 2026-12-11"),
            "Create milestone Toolchain 2.0".to_string(),
            "Seed".to_string(),
        ]
    );
    // Closed milestones leave the default list.
    let open = p.tool(json!({ "action": "list" }));
    assert_eq!(open["milestones"], json!([]));
    assert_eq!(open["registry_sync"]["state"], "synced");
    let all = p.tool(json!({ "action": "list", "all": true }));
    assert_eq!(all["milestones"][0]["id"], id.as_str());
}

#[test]
fn hs2_9pyncn_serverless_refusals_carry_their_codes_and_write_nothing() {
    let p = project();
    let id = p.tool(json!({ "action": "create", "title": "Alpha" }))["milestone"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    p.refused(
        json!({ "action": "create", "title": "alpha" }),
        "milestone_title_taken",
    );
    p.refused(
        json!({ "action": "create", "title": "" }),
        "missing required argument 'title'",
    );
    p.refused(
        json!({ "action": "create", "title": "a\nb" }),
        "invalid_milestone_title",
    );
    p.refused(
        json!({ "action": "create", "title": "Dated", "due_on": "2026-13-01" }),
        "invalid_due_on",
    );
    p.refused(
        json!({ "action": "show", "milestone": "Missing" }),
        "milestone_not_found",
    );
    p.refused(
        json!({ "action": "close", "milestone": id, "reason": "won" }),
        "invalid_milestone_close_reason",
    );
    p.refused(
        json!({ "action": "edit", "milestone": id, "due_on": 7 }),
        "due_on must be a string or null",
    );
    p.refused(json!({ "action": "split" }), "unknown action 'split'");
    // A conflicted file refuses writes and is left exactly as it was.
    let file = p.root.join(format!(".hotsheet2/milestones/{id}.json"));
    std::fs::write(&file, "{ \"title\": \"Alpha\",\n=======\n").unwrap();
    let shown = p.tool(json!({ "action": "show", "milestone": id }));
    assert_eq!(shown["state"], "conflicted");
    p.refused(
        json!({ "action": "reopen", "milestone": id }),
        "milestone_registry_conflict",
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "{ \"title\": \"Alpha\",\n=======\n"
    );
    assert_eq!(p.remote_subjects(), vec!["Create milestone Alpha", "Seed"]);
}

/// Records each request so the forwarded route and body can be asserted.
#[derive(Default)]
struct Recording {
    calls: RefCell<Vec<String>>,
}

impl Backend for Recording {
    fn get(&self, path: &str, query: &[(String, String)]) -> Result<Value, BackendError> {
        let query: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
        self.calls
            .borrow_mut()
            .push(format!("GET {path} {}", query.join("&")));
        Ok(json!({}))
    }
    fn send(&self, method: &str, path: &str, body: &Value) -> Result<Value, BackendError> {
        self.calls
            .borrow_mut()
            .push(format!("{method} {path} {body}"));
        Ok(json!({}))
    }
}

#[test]
fn hs2_9pyncn_milestone_tool_forwards_the_server_routes() {
    let backend = Recording::default();
    let tool = |args: Value| call(&backend, "hotsheet_milestones", args);
    tool(json!({ "checkout": "project" }));
    tool(json!({ "checkout": "project", "all": true }));
    tool(json!({ "checkout": "a/b", "action": "show", "milestone": "Toolchain 2.0" }));
    tool(
        json!({ "checkout": "project", "action": "create", "title": "T", "due_on": "2026-12-04" }),
    );
    tool(
        json!({ "checkout": "project", "action": "edit", "milestone": "7K3M9P2QXA", "due_on": null, "title": "U" }),
    );
    tool(
        json!({ "checkout": "project", "action": "close", "milestone": "7K3M9P2QXA", "reason": "not_planned" }),
    );
    tool(json!({ "checkout": "project", "action": "reopen", "milestone": "7K3M9P2QXA" }));
    let calls = backend.calls.borrow();
    assert_eq!(calls[0], "GET /checkouts/project/milestones ");
    assert_eq!(calls[1], "GET /checkouts/project/milestones all=true");
    assert_eq!(calls[2], "GET /checkouts/a%2Fb/milestones/Toolchain%202.0 ");
    let body =
        |call: &str| -> Value { serde_json::from_str(&call[call.find('{').unwrap()..]).unwrap() };
    assert!(calls[3].starts_with("POST /checkouts/project/milestones "));
    assert_eq!(
        body(&calls[3]),
        json!({ "title": "T", "due_on": "2026-12-04", "actor": { "role": "human", "id": "mcp-test" } })
    );
    assert!(calls[4].starts_with("PATCH /checkouts/project/milestones/7K3M9P2QXA "));
    assert_eq!(
        body(&calls[4]),
        json!({ "title": "U", "due_on": null, "actor": { "role": "human", "id": "mcp-test" } })
    );
    assert!(calls[5].starts_with("POST /checkouts/project/milestones/7K3M9P2QXA/close "));
    assert_eq!(body(&calls[5])["reason"], "not_planned");
    assert!(calls[6].starts_with("POST /checkouts/project/milestones/7K3M9P2QXA/reopen "));
    drop(calls);

    // Without a checkout there is no registry: refused before any request.
    let before = backend.calls.borrow().len();
    for args in [json!({}), json!({ "action": "create", "title": "T" })] {
        let refused = tool(args);
        assert!(
            refused["error"]
                .as_str()
                .unwrap()
                .contains("milestone_registry_required"),
            "{refused}"
        );
    }
    assert_eq!(backend.calls.borrow().len(), before);
}

#[test]
fn hs2_9pyncn_tool_list_describes_milestones() {
    let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let reply = handle_message(&request, &Recording::default()).unwrap();
    let tool = reply["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "hotsheet_milestones")
        .expect("hotsheet_milestones is listed")
        .clone();
    assert_eq!(
        tool["inputSchema"]["properties"]["action"]["enum"],
        json!(["list", "show", "create", "edit", "close", "reopen"])
    );
    for name in [
        "milestone",
        "title",
        "description",
        "due_on",
        "reason",
        "all",
        "checkout",
        "actor_role",
    ] {
        assert!(
            tool["inputSchema"]["properties"][name].is_object(),
            "{name}"
        );
    }
    let description = tool["description"].as_str().unwrap();
    assert!(description.contains("milestone_registry_conflict"));
}
