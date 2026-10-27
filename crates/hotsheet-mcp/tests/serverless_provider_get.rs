//! HS2-G78TZ9: the shipped serverless `hotsheet-mcp --path` process answers a qualified
//! `hotsheet_get` for its own Git connection with the full ticket, and refuses a foreign
//! connection with an explicit 409 instead of `unsupported GET`.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::{Value, json};

#[test]
fn hs2_g78tz9_serverless_process_reads_a_local_connection_ticket() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("ticket-store");
    FsStore::init(&store, &StoreMetadata::new("HS")).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_hotsheet-mcp"))
        .arg("--path")
        .arg(&store)
        .current_dir(root.path())
        .env("HOTSHEET_HOME", root.path().join("home"))
        .env_remove("HOTSHEET_STORE")
        .env_remove("HOTSHEET_SERVER")
        .env_remove("HOTSHEET_SECRET")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut next_id = 0;
    // The closure owns the pipes, so dropping it closes stdin and ends the session.
    let mut tool = move |name: &str, arguments: Value| -> Value {
        next_id += 1;
        let request = json!({"jsonrpc":"2.0","id":next_id,"method":"tools/call",
                             "params":{"name":name,"arguments":arguments}});
        writeln!(stdin, "{request}").unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["id"], next_id, "{reply}");
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        serde_json::from_str(text).unwrap_or_else(|_| json!({"error": text}))
    };

    let providers = tool("hotsheet_providers", json!({}));
    let connection = providers[0]["connection_id"].as_str().unwrap().to_owned();
    let created = tool(
        "hotsheet_create",
        json!({"title":"Qualified read","details":"synthetic body",
               "actor_role":"human","actor_id":"mcp-test"}),
    );
    let id = created["id"].as_str().unwrap().to_owned();
    tool(
        "hotsheet_update",
        json!({"id":id,"note":"synthetic note","actor_role":"human","actor_id":"mcp-test"}),
    );

    let got = tool(
        "hotsheet_get",
        json!({"connection":connection,"id":created["slug"]}),
    );
    assert_eq!(got["details"], "synthetic body", "{got}");
    assert_eq!(got["notes"][0]["text"], "synthetic note");
    assert_eq!(got["qualified_id"], format!("{connection}:{id}"));

    let foreign = tool("hotsheet_get", json!({"connection":"github-main","id":id}));
    assert_eq!(
        foreign["error"],
        "error 409: provider connection 'github-main' is not available in the serverless MCP backend"
    );

    drop(tool);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
