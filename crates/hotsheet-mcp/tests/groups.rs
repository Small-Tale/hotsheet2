//! Ticket groups over MCP (HS2-PDSHNN, HS2-44W3TP, HS2-8CDFKR): the `group` field on
//! create/update/batch, the `group`/`has_group` query filters, and `hotsheet_groups`,
//! both serverless (real stores) and forwarded to a server (the routes it must call).

use std::cell::RefCell;

use hotsheet_mcp::{Backend, BackendError, CoreBackend, handle_message};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::{Value, json};

fn call(backend: &dyn Backend, name: &str, mut args: Value) -> Value {
    // Independent of any actor role the test process inherited.
    let object = args.as_object_mut().expect("tool arguments are an object");
    if name != "hotsheet_query" && name != "hotsheet_get" {
        object.entry("actor_role").or_insert_with(|| json!("human"));
        object
            .entry("actor_id")
            .or_insert_with(|| json!("mcp-test"));
    }
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

fn core() -> (tempfile::TempDir, CoreBackend) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init_git(dir.path(), &StoreMetadata::new("HS")).unwrap();
    (dir, CoreBackend::new(store))
}

fn slugs(rows: &Value) -> Vec<String> {
    let mut slugs: Vec<String> = rows
        .as_array()
        .unwrap_or_else(|| panic!("rows: {rows}"))
        .iter()
        .map(|row| row["slug"].as_str().unwrap().to_owned())
        .collect();
    slugs.sort();
    slugs
}

#[test]
fn hs2_pdshnn_serverless_group_field_filters_and_tool() {
    let (_dir, backend) = core();
    let first = call(
        &backend,
        "hotsheet_create",
        json!({ "title": "first", "group": "  Board   Drag " }),
    );
    assert_eq!(first["group"], "Board Drag");
    let second = call(&backend, "hotsheet_create", json!({ "title": "second" }));
    assert_eq!(second["group"], Value::Null);
    let third = call(
        &backend,
        "hotsheet_create",
        json!({ "title": "third", "group": "alpha" }),
    );
    let slug = |ticket: &Value| ticket["slug"].as_str().unwrap().to_owned();

    // An existing group's spelling is reused; an unrelated update leaves the group alone.
    let joined = call(
        &backend,
        "hotsheet_update",
        json!({ "id": slug(&second), "group": "BOARD drag" }),
    );
    assert_eq!(joined["group"], "Board Drag");
    let retitled = call(
        &backend,
        "hotsheet_update",
        json!({ "id": slug(&second), "title": "second again" }),
    );
    assert_eq!(retitled["group"], "Board Drag");
    assert_eq!(
        call(&backend, "hotsheet_get", json!({ "id": slug(&second) }))["group"],
        "Board Drag"
    );

    // Invalid input is a tool error and writes nothing.
    for (bad, message) in [
        (json!(7), "group must be a string or null"),
        (json!("line\nbreak"), "control characters"),
        (json!("x".repeat(121)), "120 characters"),
    ] {
        let refused = call(
            &backend,
            "hotsheet_update",
            json!({ "id": slug(&second), "group": bad }),
        );
        assert!(
            refused["error"].as_str().unwrap().contains(message),
            "{refused}"
        );
    }
    assert!(
        call(
            &backend,
            "hotsheet_create",
            json!({ "title": "bad", "group": "a\tb" })
        )["error"]
            .is_string()
    );

    // Query filters and the row field.
    let grouped = call(
        &backend,
        "hotsheet_query",
        json!({ "group": "board   DRAG" }),
    );
    assert_eq!(slugs(&grouped), {
        let mut expected = vec![slug(&first), slug(&second)];
        expected.sort();
        expected
    });
    assert!(
        grouped
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["group"] == "Board Drag")
    );
    assert_eq!(
        slugs(&call(
            &backend,
            "hotsheet_query",
            json!({ "has_group": true })
        ))
        .len(),
        3
    );
    assert!(
        slugs(&call(
            &backend,
            "hotsheet_query",
            json!({ "has_group": false })
        ))
        .is_empty()
    );
    assert!(
        slugs(&call(
            &backend,
            "hotsheet_query",
            json!({ "group": "nope" })
        ))
        .is_empty()
    );
    let projected = call(
        &backend,
        "hotsheet_query",
        json!({ "fields": "group", "group": "alpha" }),
    );
    assert_eq!(projected[0]["group"], "alpha");

    // The groups tool: list (the default action), rename, ungroup.
    let expected = json!({ "groups": [
        { "name": "alpha", "count": 1 },
        { "name": "Board Drag", "count": 2 },
    ] });
    assert_eq!(call(&backend, "hotsheet_groups", json!({})), expected);
    assert_eq!(
        call(&backend, "hotsheet_groups", json!({ "action": "list" })),
        expected
    );
    let missing = call(
        &backend,
        "hotsheet_groups",
        json!({ "action": "rename", "from": "missing", "to": "x" }),
    );
    assert!(
        missing["error"]
            .as_str()
            .unwrap()
            .contains("no group named 'missing'")
    );
    let taken = call(
        &backend,
        "hotsheet_groups",
        json!({ "action": "rename", "from": "alpha", "to": "board drag" }),
    );
    assert!(
        taken["error"]
            .as_str()
            .unwrap()
            .contains("a group named 'Board Drag' already exists")
    );
    assert!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "action": "rename", "from": "alpha" })
        )["error"]
            .is_string(),
        "a rename needs both names"
    );
    assert!(
        call(&backend, "hotsheet_groups", json!({ "action": "merge" }))["error"]
            .as_str()
            .unwrap()
            .contains("unknown action")
    );
    assert_eq!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "action": "rename", "from": "BOARD DRAG", "to": "board drag" }),
        ),
        json!({ "name": "board drag", "updated": 2 }),
        "a case-only rename of the same group is allowed"
    );
    assert_eq!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "action": "rename", "from": "board drag", "to": "Sprint 1" }),
        ),
        json!({ "name": "Sprint 1", "updated": 2 })
    );
    assert_eq!(
        call(&backend, "hotsheet_get", json!({ "id": slug(&first) }))["group"],
        "Sprint 1"
    );

    // Batch regroup, then clear with null on one ticket.
    let batch = call(
        &backend,
        "hotsheet_batch",
        json!({ "ids": [slug(&first), slug(&third)], "group": "Merged" }),
    );
    assert_eq!(batch["updated"].as_array().unwrap().len(), 2, "{batch}");
    let cleared = call(
        &backend,
        "hotsheet_update",
        json!({ "id": slug(&third), "group": null }),
    );
    assert_eq!(cleared["group"], Value::Null);
    assert_eq!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "action": "ungroup", "name": "sprint 1" })
        ),
        json!({ "updated": 1 })
    );
    assert!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "action": "ungroup", "name": "sprint 1" })
        )["error"]
            .as_str()
            .unwrap()
            .contains("no group named")
    );
    assert_eq!(
        call(&backend, "hotsheet_groups", json!({})),
        json!({ "groups": [{ "name": "Merged", "count": 1 }] })
    );
}

/// A checkout is one group namespace across its git stores, as on the server.
#[test]
fn hs2_pdshnn_serverless_checkout_shares_one_group_namespace() {
    let root = tempfile::tempdir().unwrap();
    let checkout_root = root.path().join("project");
    std::fs::create_dir(&checkout_root).unwrap();
    let store_roots: Vec<_> = ["AA", "BB"]
        .into_iter()
        .map(|prefix| {
            let store_root = root.path().join(format!("{prefix}.hs2"));
            FsStore::init_git(&store_root, &StoreMetadata::new(prefix)).unwrap();
            store_root
        })
        .collect();
    let registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(
        root.path().join("home/checkouts.json"),
    );
    let checkout = registry
        .register(&checkout_root, Some("project"), None, store_roots.clone())
        .unwrap();
    let backend =
        CoreBackend::new(FsStore::open(&store_roots[0]).unwrap()).with_checkout_registry(registry);
    let create = |store: &std::path::Path, title: &str, group: Option<&str>| {
        // The store selector is the linked store's root exactly as the checkout records it.
        let linked = checkout
            .stores
            .iter()
            .find(|linked| {
                std::path::Path::new(linked.as_str()).canonicalize().ok()
                    == store.canonicalize().ok()
            })
            .unwrap();
        let mut args = json!({ "checkout": checkout.id, "store": linked, "title": title });
        if let Some(group) = group {
            args["group"] = json!(group);
        }
        call(&backend, "hotsheet_create", args)
    };
    let a = create(&store_roots[0], "in the first store", Some("Sprint"));
    assert_eq!(a["group"], "Sprint", "{a}");
    // The second store reuses the spelling the first store already has.
    let b = create(&store_roots[1], "in the second store", Some("  SPRINT "));
    assert_eq!(b["group"], "Sprint", "{b}");
    let c = create(&store_roots[1], "loose", None);
    let joined = call(
        &backend,
        "hotsheet_update",
        json!({ "checkout": checkout.id, "id": c["slug"], "group": "sprint" }),
    );
    assert_eq!(joined["group"], "Sprint", "{joined}");

    assert_eq!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "checkout": checkout.id })
        ),
        json!({ "groups": [{ "name": "Sprint", "count": 3 }] })
    );
    let rows = call(
        &backend,
        "hotsheet_query",
        json!({ "checkout": checkout.id, "group": "sprint" }),
    );
    assert_eq!(slugs(&rows).len(), 3);

    assert_eq!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "checkout": checkout.id, "action": "rename", "from": "sprint", "to": "Sprint 2" }),
        ),
        json!({ "name": "Sprint 2", "updated": 3 })
    );
    for root in &store_roots {
        let store = FsStore::open(root).unwrap();
        assert!(
            store
                .list_tickets()
                .unwrap()
                .iter()
                .all(|ticket| ticket.group.as_deref() == Some("Sprint 2"))
        );
    }
    assert_eq!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "checkout": checkout.id, "action": "ungroup", "name": "SPRINT 2" }),
        ),
        json!({ "updated": 3 })
    );
    assert_eq!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "checkout": checkout.id })
        ),
        json!({ "groups": [] })
    );
    // A connection the serverless backend does not host refuses explicitly.
    let foreign = call(
        &backend,
        "hotsheet_groups",
        json!({ "connection": "jira-eng" }),
    );
    assert!(
        foreign["error"]
            .as_str()
            .unwrap()
            .contains("provider connection 'jira-eng'"),
        "{foreign}"
    );
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
        Ok(json!([]))
    }
    fn send(&self, method: &str, path: &str, body: &Value) -> Result<Value, BackendError> {
        self.calls
            .borrow_mut()
            .push(format!("{method} {path} {body}"));
        Ok(json!({}))
    }
}

#[test]
fn hs2_pdshnn_group_tools_forward_the_server_routes() {
    let backend = Recording::default();
    call(&backend, "hotsheet_groups", json!({}));
    call(
        &backend,
        "hotsheet_groups",
        json!({ "checkout": "project" }),
    );
    call(
        &backend,
        "hotsheet_groups",
        json!({ "connection": "git-main" }),
    );
    call(
        &backend,
        "hotsheet_groups",
        json!({ "checkout": "project", "action": "rename", "from": "Old", "to": "New" }),
    );
    call(
        &backend,
        "hotsheet_groups",
        json!({ "checkout": "project", "action": "ungroup", "name": "Old" }),
    );
    call(
        &backend,
        "hotsheet_query",
        json!({ "checkout": "project", "group": "Old", "has_group": true }),
    );
    call(
        &backend,
        "hotsheet_update",
        json!({ "checkout": "project", "id": "HS-1", "group": null }),
    );
    call(
        &backend,
        "hotsheet_create",
        json!({ "checkout": "project", "title": "t", "group": "Old" }),
    );
    let calls = backend.calls.borrow();
    assert_eq!(calls[0], "GET /groups ");
    assert_eq!(calls[1], "GET /checkouts/project/groups ");
    assert_eq!(calls[2], "GET /providers/git-main/groups ");
    assert!(
        calls[3].starts_with("POST /checkouts/project/groups/rename "),
        "{}",
        calls[3]
    );
    let rename: Value = serde_json::from_str(&calls[3][calls[3].find('{').unwrap()..]).unwrap();
    assert_eq!(
        rename,
        json!({ "from": "Old", "to": "New", "actor": { "role": "human", "id": "mcp-test" } })
    );
    assert!(calls[4].starts_with("POST /checkouts/project/groups/ungroup "));
    assert!(calls[4].contains("\"name\":\"Old\""));
    assert!(calls[5].starts_with("GET /checkouts/project/tickets "));
    assert!(calls[5].contains("group=Old") && calls[5].contains("has_group=true"));
    assert!(calls[6].starts_with("PATCH /checkouts/project/tickets/HS-1 "));
    assert!(calls[6].contains("\"group\":null"));
    assert!(calls[7].starts_with("POST /checkouts/project/tickets "));
    assert!(calls[7].contains("\"group\":\"Old\""));
}

#[test]
fn hs2_pdshnn_tool_list_describes_groups() {
    let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let reply = handle_message(&request, &Recording::default()).unwrap();
    let tools = reply["result"]["tools"].as_array().unwrap();
    let tool = |name: &str| {
        tools
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("missing tool {name}"))
    };
    let groups = tool("hotsheet_groups");
    assert_eq!(
        groups["inputSchema"]["properties"]["action"]["enum"],
        json!(["list", "rename", "ungroup"])
    );
    for name in ["from", "to", "name", "checkout", "connection"] {
        assert!(
            groups["inputSchema"]["properties"][name].is_object(),
            "{name}"
        );
    }
    assert_eq!(
        tool("hotsheet_update")["inputSchema"]["properties"]["group"]["type"],
        json!(["string", "null"])
    );
    assert!(tool("hotsheet_create")["inputSchema"]["properties"]["group"].is_object());
    assert!(tool("hotsheet_batch")["inputSchema"]["properties"]["group"].is_object());
    let query = &tool("hotsheet_query")["inputSchema"]["properties"];
    assert_eq!(query["has_group"]["type"], "boolean");
    assert!(query["group"].is_object());
}
