//! HS2-DXD7DZ: `hotsheet_query` through the MCP shim's HTTP backend against a real server
//! socket honors `compact` and the `fields` allow-list for a qualified connection, the same
//! route a project-launched `hotsheet-mcp --project` session uses. Synthetic tickets carry
//! large bodies, notes and attachment metadata so a dropped projection is unmistakable.

use hotsheet_mcp::{HttpBackend, handle_message};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use tokio::net::TcpListener;

const SECRET: &str = "test-secret";
const FIVE: &str = "slug,title,status,priority,up_next";
const TICKETS: usize = 6;
const NOTES: usize = 4;

fn request(method: &str, url: &str, body: Option<&str>, filename: Option<&str>) -> Value {
    let mut request = ureq::request(method, url).set("x-hotsheet-secret", SECRET);
    if let Some(filename) = filename {
        request = request.set("x-hotsheet-filename", filename);
    } else {
        request = request.set("content-type", "application/json");
    }
    let response = match body {
        Some(body) => request.send_string(body).unwrap(),
        None => request.call().unwrap(),
    };
    serde_json::from_str(&response.into_string().unwrap()).unwrap()
}

/// One MCP `tools/call` through the shim's real dispatch and HTTP transport; returns the
/// decoded tool result and its text length (the payload an agent receives).
fn mcp(base: &str, arguments: Value) -> (Value, usize) {
    let backend = HttpBackend::new(base, SECRET);
    let reply = handle_message(
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name": if arguments.get("id").is_some() { "hotsheet_get" } else { "hotsheet_query" },
                          "arguments": arguments}}),
        &backend,
    )
    .unwrap();
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    (serde_json::from_str(text).unwrap(), text.len())
}

fn keys(row: &Value) -> BTreeSet<&str> {
    row.as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect()
}

#[tokio::test]
async fn hs2_dxd7dz_connection_query_honors_compact_and_fields_through_the_mcp_shim() {
    let directory = tempfile::tempdir().unwrap();
    let store = FsStore::init(directory.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_default_codex_home(directory.path().join("no-codex-home"));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });

    let base_for_checks = base.clone();
    tokio::task::spawn_blocking(move || {
        let base = base_for_checks;
        let body = "synthetic body ".repeat(256).trim_end().to_owned();
        let note = "synthetic note ".repeat(128).trim_end().to_owned();
        let mut ids = Vec::new();
        for index in 0..TICKETS {
            let created = request(
                "POST",
                &format!("{base}/tickets"),
                Some(&json!({"title": format!("synthetic {index}"), "details": body}).to_string()),
                None,
            );
            let id = created["id"].as_str().unwrap().to_owned();
            for _ in 0..NOTES {
                request(
                    "PATCH",
                    &format!("{base}/tickets/{id}"),
                    Some(&json!({"note": note}).to_string()),
                    None,
                );
            }
            request(
                "POST",
                &format!("{base}/tickets/{id}/attachments"),
                Some("synthetic attachment bytes"),
                Some("evidence.txt"),
            );
            ids.push(id);
        }
        request(
            "PATCH",
            &format!("{base}/tickets/{}", ids[0]),
            Some(r#"{"status":"started"}"#),
            None,
        );
        let providers = request("GET", &format!("{base}/providers"), None, None);
        let connection = providers[0]["connection_id"].as_str().unwrap().to_owned();

        // The observed call: an explicit compact five-field projection.
        let (projected, projected_len) = mcp(
            &base,
            json!({"connection": connection, "compact": true, "fields": FIVE}),
        );
        let rows = projected.as_array().unwrap();
        assert_eq!(rows.len(), TICKETS);
        for row in rows {
            assert_eq!(
                keys(row),
                ["priority", "slug", "status", "title", "up_next"].into()
            );
        }

        // The tool's documented compact default: the shared row, no body or history.
        let (compact, compact_len) = mcp(&base, json!({"connection": connection}));
        for row in compact.as_array().unwrap() {
            for omitted in ["details", "notes", "attachments", "claim_history"] {
                assert!(row.get(omitted).is_none(), "compact row omits {omitted}");
            }
            assert_eq!(
                row["qualified_id"],
                format!("{connection}:{}", row["native_id"].as_str().unwrap())
            );
        }

        // Deliberate full reads stay available: compact=false and hotsheet_get.
        let (full, full_len) = mcp(&base, json!({"connection": connection, "compact": false}));
        for row in full.as_array().unwrap() {
            assert_eq!(row["details"], body);
            assert!(row["notes"].as_array().unwrap().len() >= NOTES);
            assert_eq!(row["attachments"][0]["filename"], "evidence.txt");
        }
        let (one, _) = mcp(&base, json!({"connection": connection, "id": ids[1]}));
        assert_eq!(one["details"], body);
        assert!(one["notes"].as_array().unwrap().len() >= NOTES);
        assert_eq!(one["attachments"].as_array().unwrap().len(), 1);
        assert!(
            projected_len * 20 < full_len && compact_len * 5 < full_len,
            "projection shrinks the payload: {projected_len} / {compact_len} / {full_len}"
        );

        // The provider route's HTTP default is unchanged for the web client: full tickets.
        let direct = request(
            "GET",
            &format!("{base}/providers/{connection}/tickets"),
            None,
            None,
        );
        assert!(direct[0]["notes"].as_array().unwrap().len() >= NOTES);
        assert_eq!(direct[0]["details"], body);

        // Filters and keyset paging still apply under a projection.
        let (started, _) = mcp(
            &base,
            json!({"connection": connection, "status": "started", "fields": FIVE}),
        );
        assert_eq!(started.as_array().unwrap().len(), 1);
        assert_eq!(started[0]["status"], "started");
        let mut seen = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let mut arguments =
                json!({"connection": connection, "sort": "id", "limit": 4, "fields": "id"});
            if let Some(after) = &after {
                arguments["page_after"] = json!(after);
            }
            let (page, _) = mcp(&base, arguments);
            let page = page.as_array().unwrap().clone();
            for row in &page {
                assert_eq!(keys(row), ["id", "slug"].into());
            }
            seen.extend(
                page.iter()
                    .map(|row| row["id"].as_str().unwrap().to_owned()),
            );
            if page.len() < 4 {
                break;
            }
            after = Some(seen.last().unwrap().clone());
        }
        let mut expected = ids.clone();
        expected.sort();
        assert_eq!(seen, expected, "keyset pages visit every ticket once");
    })
    .await
    .unwrap();
}

/// One `hotsheet_query` through any MCP backend; returns the slugs in the order received.
fn slugs(backend: &dyn hotsheet_mcp::Backend, arguments: &Value) -> Vec<String> {
    let reply = handle_message(
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name":"hotsheet_query","arguments": arguments}}),
        backend,
    )
    .unwrap();
    assert_eq!(reply["result"]["isError"], false, "{arguments}: {reply}");
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    serde_json::from_str::<Value>(text)
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["slug"].as_str().unwrap().to_owned())
        .collect()
}

/// HS2-DXD7DZ review return: the serverless backend's local-connection list applies the
/// `created_*`/`updated_*` ranges exactly as the server does (inclusive lexical bounds, a
/// malformed value compared as text), in compact, projected and full shapes and with paging.
#[tokio::test]
async fn hs2_dxd7dz_serverless_connection_date_ranges_match_the_server() {
    let directory = tempfile::tempdir().unwrap();
    let store = FsStore::init(directory.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_default_codex_home(directory.path().join("no-codex-home"));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });
    let root = directory.path().to_path_buf();

    tokio::task::spawn_blocking(move || {
        let mut tickets = Vec::new();
        for index in 0..5 {
            tickets.push(request(
                "POST",
                &format!("{base}/tickets"),
                Some(&json!({"title": format!("range {index}")}).to_string()),
                None,
            ));
        }
        // Touch the first ticket last, so its updated_at is the newest.
        let touched = request(
            "PATCH",
            &format!("{base}/tickets/{}", tickets[0]["id"].as_str().unwrap()),
            Some(r#"{"note":"later edit"}"#),
            None,
        );
        let slug = |ticket: &Value| ticket["slug"].as_str().unwrap().to_owned();
        let created = |index: usize| tickets[index]["created_at"].as_str().unwrap().to_owned();
        let connection =
            request("GET", &format!("{base}/providers"), None, None)[0]["connection_id"]
                .as_str()
                .unwrap()
                .to_owned();
        let http = HttpBackend::new(&base, SECRET);
        let serverless = hotsheet_mcp::CoreBackend::open(&root).unwrap();

        let sorted = |mut values: Vec<String>| {
            values.sort();
            values
        };
        let expect =
            |indices: &[usize]| sorted(indices.iter().map(|i| slug(&tickets[*i])).collect());
        let touched_at = touched["updated_at"].as_str().unwrap().to_owned();
        let cases: Vec<(Value, Option<Vec<String>>)> = vec![
            (
                json!({"created_after": created(2)}),
                Some(expect(&[2, 3, 4])),
            ),
            (
                json!({"created_before": created(2)}),
                Some(expect(&[0, 1, 2])),
            ),
            (
                json!({"created_after": created(1), "created_before": created(3)}),
                Some(expect(&[1, 2, 3])),
            ),
            (json!({"updated_after": touched_at}), Some(expect(&[0]))),
            (
                json!({"updated_before": created(4)}),
                Some(expect(&[1, 2, 3, 4])),
            ),
            (
                json!({"created_after": "2999-01-01T00:00:00Z"}),
                Some(Vec::new()),
            ),
            (
                json!({"created_before": "1999-01-01T00:00:00Z"}),
                Some(Vec::new()),
            ),
            // Malformed bounds are compared as text on the server; the serverless backend agrees.
            (json!({"created_after": "not-a-date"}), None),
            (json!({"updated_before": "not-a-date"}), None),
        ];
        for (range, expected) in cases {
            for shape in [
                json!({}),
                json!({"compact": true, "fields": FIVE}),
                json!({"compact": false}),
            ] {
                let mut arguments = range.clone();
                arguments["connection"] = json!(connection);
                for (key, value) in shape.as_object().unwrap() {
                    arguments[key] = value.clone();
                }
                let over_http = sorted(slugs(&http, &arguments));
                let local = sorted(slugs(&serverless, &arguments));
                assert_eq!(local, over_http, "serverless/HTTP parity for {arguments}");
                if let Some(expected) = &expected {
                    assert_eq!(&over_http, expected, "{arguments}");
                }
            }
        }

        // Keyset paging under a range visits the same rows on both backends.
        for backend in [&http as &dyn hotsheet_mcp::Backend, &serverless] {
            let mut seen = Vec::new();
            let mut after: Option<String> = None;
            loop {
                let mut arguments = json!({"connection": connection, "created_after": created(1),
                                           "sort": "id", "limit": 2, "fields": "id"});
                if let Some(after) = &after {
                    arguments["page_after"] = json!(after);
                }
                let reply = handle_message(
                    &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                            "params":{"name":"hotsheet_query","arguments": arguments}}),
                    backend,
                )
                .unwrap();
                let page: Value =
                    serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap())
                        .unwrap();
                let ids: Vec<String> = page
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|row| row["id"].as_str().unwrap().to_owned())
                    .collect();
                seen.extend(ids.iter().cloned());
                if ids.len() < 2 {
                    break;
                }
                after = ids.last().cloned();
            }
            let mut expected: Vec<String> = tickets[1..]
                .iter()
                .map(|ticket| ticket["id"].as_str().unwrap().to_owned())
                .collect();
            expected.sort();
            assert_eq!(seen, expected);
        }
    })
    .await
    .unwrap();
}
