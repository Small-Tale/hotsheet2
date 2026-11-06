//! HS2-FF9ZMS: `hotsheet_create` never infers `feedback_needed` or `needs_review` from details
//! prose that merely mentions the feedback marker. Only a line that begins with the marker is a
//! details request; a later edit to prose clears it, with no stored flag to reset.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[path = "support/git.rs"]
mod git_fixture;

use hotsheet_ticketing::StoreMetadata;
use serde_json::{Value, json};

#[test]
fn hs2_ff9zms_create_flags_follow_only_a_line_start_details_marker() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let store = root.path().join("demo.hs2");
    git_fixture::init(&store, &StoreMetadata::new("HS"));
    std::fs::create_dir_all(&home).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_hotsheet-mcp"))
        .arg("--path")
        .arg(&store)
        .current_dir(root.path())
        .env("HOTSHEET_HOME", &home)
        .env_remove("HOTSHEET_STORE")
        .env_remove("HOTSHEET_SERVER")
        .env_remove("HOTSHEET_SECRET")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut next_id = 0;
    let mut tool = move |name: &str, mut arguments: Value| -> Value {
        next_id += 1;
        if name != "hotsheet_get" {
            arguments["actor_role"] = json!("human");
            arguments["actor_id"] = json!("mcp-test");
        }
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
    let flags = |ticket: &Value| {
        (
            ticket["feedback_needed"].as_bool(),
            ticket["needs_review"].as_bool(),
        )
    };

    // The HS2-JTTQMY shape, synthetic: prose names the marker and the review flag mid-line.
    let prose = "Derived state: shows FEEDBACK NEEDED and the needs-review flag when a note asks.";
    let created = tool(
        "hotsheet_create",
        json!({"title": "Prose mention", "details": prose}),
    );
    assert_eq!(flags(&created), (Some(false), Some(false)), "{created}");
    let slug = created["slug"].as_str().unwrap().to_owned();
    assert_eq!(
        flags(&tool("hotsheet_get", json!({"id": slug}))),
        (Some(false), Some(false))
    );

    // A details line that begins with the marker is still a deliberate request.
    let asked = tool(
        "hotsheet_create",
        json!({"title": "Line-start request", "details": "Context.\n\nFEEDBACK NEEDED: which layout?"}),
    );
    assert_eq!(flags(&asked), (Some(true), Some(true)), "{asked}");
    let asked_slug = asked["slug"].as_str().unwrap().to_owned();
    // Rewording it to prose settles it.
    let reworded = tool(
        "hotsheet_update",
        json!({"id": asked_slug, "details": "Context. The FEEDBACK NEEDED question was settled."}),
    );
    assert!(reworded.get("error").is_none(), "{reworded}");
    assert_eq!(
        flags(&tool("hotsheet_get", json!({"id": asked_slug}))),
        (Some(false), Some(false))
    );
    drop(tool);
    let _ = child.wait();
}
