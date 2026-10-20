//! A committed MCP entry uses `hotsheet-mcp --path .` from a linked checkout.

use std::io::Write;
use std::process::{Command, Stdio};

use hotsheet_ticketing::{FsStore, StoreMetadata};

#[test]
fn portable_project_entry_opens_its_linked_store_and_lists_tools() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("ticket-store");
    FsStore::init(&store, &StoreMetadata::new("HS")).unwrap();
    let project = root.path().join("project");
    let nested = project.join("src");
    std::fs::create_dir_all(project.join(".hotsheet2")).unwrap();
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(
        project.join(".hotsheet2/store"),
        format!("{}\n", store.display()),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_hotsheet-mcp"))
        .arg("--path")
        .arg(".")
        .current_dir(&nested)
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
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{}}}}"#
    )
    .unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reply: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["id"], 1);
    assert!(reply["result"]["tools"].as_array().is_some());
}
