//! Ticket groups over MCP (HS2-PDSHNN, HS2-44W3TP, HS2-8CDFKR, HS2-9KZSD4): the `group`
//! field on create/update/batch, the `group`/`has_group` query filters, and
//! `hotsheet_groups` (list, create, rename, ungroup, merge, migrate), both serverless
//! (real stores and a checkout's group registry) and forwarded to a server (the routes it
//! must call).

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

const ID: &str = "7K3M9P2QXA";
const OTHER_ID: &str = "0000000001";

#[test]
fn hs2_9kzsd4_serverless_store_takes_group_ids_and_refuses_names() {
    let (_dir, backend) = core();
    let first = call(
        &backend,
        "hotsheet_create",
        json!({ "title": "first", "group": format!(" {ID} ") }),
    );
    assert_eq!(first["group"], ID);
    let second = call(&backend, "hotsheet_create", json!({ "title": "second" }));
    assert_eq!(second["group"], Value::Null);
    let slug = |ticket: &Value| ticket["slug"].as_str().unwrap().to_owned();
    let joined = call(
        &backend,
        "hotsheet_update",
        json!({ "id": slug(&second), "group": ID }),
    );
    assert_eq!(joined["group"], ID);
    let retitled = call(
        &backend,
        "hotsheet_update",
        json!({ "id": slug(&second), "title": "second again" }),
    );
    assert_eq!(retitled["group"], ID);

    // Invalid input and names without a project are tool errors that write nothing.
    for (bad, message) in [
        (json!(7), "group must be a string or null"),
        (json!("line\nbreak"), "control characters"),
        (json!("x".repeat(121)), "120 characters"),
        (json!("Sprint"), "group names need a project checkout"),
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
    assert_eq!(
        call(&backend, "hotsheet_get", json!({ "id": slug(&second) }))["group"],
        ID
    );

    let grouped = call(&backend, "hotsheet_query", json!({ "group": ID }));
    assert_eq!(slugs(&grouped), {
        let mut expected = vec![slug(&first), slug(&second)];
        expected.sort();
        expected
    });
    assert!(
        slugs(&call(
            &backend,
            "hotsheet_query",
            json!({ "group": "Sprint" })
        ))
        .is_empty()
    );
    assert!(
        slugs(&call(
            &backend,
            "hotsheet_query",
            json!({ "has_group": false })
        ))
        .is_empty()
    );
    let projected = call(
        &backend,
        "hotsheet_query",
        json!({ "fields": "group", "group": ID }),
    );
    assert_eq!(projected[0]["group"], ID);

    // The groups tool lists ids under placeholders and refuses what needs names.
    let expected = json!({ "registry": false, "groups": [{
        "id": ID, "name": format!("Group {ID}"), "count": 2,
        "named": false, "legacy": false, "duplicate": false,
    }] });
    assert_eq!(call(&backend, "hotsheet_groups", json!({})), expected);
    assert_eq!(
        call(&backend, "hotsheet_groups", json!({ "action": "list" })),
        expected
    );
    for args in [
        json!({ "action": "rename", "group": ID, "to": "x" }),
        json!({ "action": "create", "name": "x" }),
        json!({ "action": "migrate" }),
    ] {
        assert!(
            call(&backend, "hotsheet_groups", args.clone())["error"]
                .as_str()
                .unwrap()
                .contains("group registry"),
            "{args}"
        );
    }
    assert!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "action": "rename", "group": ID })
        )["error"]
            .is_string(),
        "a rename needs both arguments"
    );
    assert!(
        call(&backend, "hotsheet_groups", json!({ "action": "split" }))["error"]
            .as_str()
            .unwrap()
            .contains("unknown action")
    );
    // Batch to another id, merge back, then ungroup (the old `name` argument still works).
    let batch = call(
        &backend,
        "hotsheet_batch",
        json!({ "ids": [slug(&second)], "group": OTHER_ID }),
    );
    assert_eq!(batch["updated"].as_array().unwrap().len(), 1, "{batch}");
    assert_eq!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "action": "merge", "from": OTHER_ID, "into": ID })
        ),
        json!({ "id": ID, "name": format!("Group {ID}"), "updated": 1 })
    );
    assert_eq!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "action": "ungroup", "name": ID })
        ),
        json!({ "updated": 2 })
    );
    assert!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "action": "ungroup", "group": ID })
        )["error"]
            .as_str()
            .unwrap()
            .contains("no group named")
    );
    let cleared = call(
        &backend,
        "hotsheet_update",
        json!({ "id": slug(&first), "group": null }),
    );
    assert_eq!(cleared["group"], Value::Null);
}

/// A checkout is one group namespace across its git stores, named by its registry.
#[test]
fn hs2_9kzsd4_serverless_checkout_names_groups_through_its_registry() {
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
    // A pre-registry name in the second store, for the migration.
    let legacy = hotsheet_ticketing::ops::create(
        &FsStore::open(&store_roots[1]).unwrap(),
        hotsheet_model::Ulid::new(),
        "BB",
        hotsheet_model::Timestamp::new("2026-11-01T00:00:00Z"),
        hotsheet_ticketing::ops::NewTicket {
            title: "legacy".into(),
            category: "task".into(),
            group: Some("Old Name".into()),
            ..Default::default()
        },
    )
    .unwrap();
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
    let sprint = a["group"].as_str().unwrap().to_owned();
    assert!(hotsheet_model::is_group_id(&sprint), "{a}");
    assert!(
        checkout_root
            .join(format!(".hotsheet2/groups/{sprint}.json"))
            .is_file()
    );
    // Another spelling in the second store is the same id.
    let b = create(&store_roots[1], "in the second store", Some("  SPRINT "));
    assert_eq!(b["group"], sprint.as_str(), "{b}");
    let c = create(&store_roots[1], "loose", None);
    let joined = call(
        &backend,
        "hotsheet_update",
        json!({ "checkout": checkout.id, "id": c["slug"], "group": "sprint" }),
    );
    assert_eq!(joined["group"], sprint.as_str(), "{joined}");
    let groups = |backend: &CoreBackend| {
        call(
            backend,
            "hotsheet_groups",
            json!({ "checkout": checkout.id }),
        )
    };
    assert_eq!(
        groups(&backend),
        json!({ "registry": true, "groups": [
            { "id": "Old Name", "name": "Old Name", "count": 1,
              "named": true, "legacy": true, "duplicate": false },
            { "id": sprint, "name": "Sprint", "count": 3,
              "named": true, "legacy": false, "duplicate": false },
        ] })
    );
    let rows = call(
        &backend,
        "hotsheet_query",
        json!({ "checkout": checkout.id, "group": "SPRINT" }),
    );
    assert_eq!(slugs(&rows).len(), 3, "{rows}");

    // Migrate, then rerun.
    assert_eq!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "checkout": checkout.id, "action": "migrate" })
        ),
        json!({ "created": 1, "reused": 0, "updated": 1 })
    );
    let migrated = FsStore::open(&store_roots[1])
        .unwrap()
        .read_ticket(&legacy.id)
        .unwrap()
        .group
        .unwrap();
    assert!(hotsheet_model::is_group_id(&migrated));
    assert_eq!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "checkout": checkout.id, "action": "migrate" })
        ),
        json!({ "created": 0, "reused": 0, "updated": 0 })
    );

    // Create, rename (registry only), merge, ungroup.
    assert_eq!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "checkout": checkout.id, "action": "create", "name": "sprint" })
        ),
        json!({ "id": sprint, "name": "Sprint", "created": false })
    );
    assert_eq!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "checkout": checkout.id, "action": "rename", "from": "sprint", "to": "Sprint 2" }),
        ),
        json!({ "id": sprint, "name": "Sprint 2", "updated": 0 })
    );
    for root in &store_roots {
        let store = FsStore::open(root).unwrap();
        assert!(
            store
                .list_tickets()
                .unwrap()
                .iter()
                .filter(|ticket| ticket.id != legacy.id)
                .all(|ticket| ticket.group.as_deref() == Some(sprint.as_str()))
        );
    }
    assert_eq!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "checkout": checkout.id, "action": "merge", "from": "old name", "into": sprint }),
        ),
        json!({ "id": sprint, "name": "Sprint 2", "updated": 1 })
    );
    assert_eq!(
        call(
            &backend,
            "hotsheet_groups",
            json!({ "checkout": checkout.id, "action": "ungroup", "group": "SPRINT 2" }),
        ),
        json!({ "updated": 4 })
    );
    assert_eq!(groups(&backend), json!({ "registry": true, "groups": [] }));
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
        "hotsheet_groups",
        json!({ "checkout": "project", "action": "create", "name": "Fresh" }),
    );
    call(
        &backend,
        "hotsheet_groups",
        json!({ "checkout": "project", "action": "merge", "from": "A", "into": "B" }),
    );
    call(
        &backend,
        "hotsheet_groups",
        json!({ "checkout": "project", "action": "migrate" }),
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
        json!({ "group": "Old", "to": "New", "actor": { "role": "human", "id": "mcp-test" } }),
        "the pre-registry `from` argument is forwarded as `group`"
    );
    assert!(calls[4].starts_with("POST /checkouts/project/groups/ungroup "));
    assert!(calls[4].contains("\"group\":\"Old\""));
    assert!(calls[5].starts_with("POST /checkouts/project/groups "));
    assert!(calls[5].contains("\"name\":\"Fresh\""));
    assert!(calls[6].starts_with("POST /checkouts/project/groups/merge "));
    assert!(calls[6].contains("\"from\":\"A\"") && calls[6].contains("\"into\":\"B\""));
    assert!(calls[7].starts_with("POST /checkouts/project/groups/migrate "));
    assert!(calls[8].starts_with("GET /checkouts/project/tickets "));
    assert!(calls[8].contains("group=Old") && calls[8].contains("has_group=true"));
    assert!(calls[9].starts_with("PATCH /checkouts/project/tickets/HS-1 "));
    assert!(calls[9].contains("\"group\":null"));
    assert!(calls[10].starts_with("POST /checkouts/project/tickets "));
    assert!(calls[10].contains("\"group\":\"Old\""));
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
        json!(["list", "create", "rename", "ungroup", "merge", "migrate"])
    );
    for name in [
        "group",
        "from",
        "into",
        "to",
        "name",
        "checkout",
        "connection",
    ] {
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
