//! HS2-QR1K8R end to end through the server: a genuine ticket-store conflict found by the
//! background sync loop starts exactly one AI session, of the project's default tool,
//! which receives the repair prompt; repeated passes start nothing more, and the health
//! contract says the AI was asked. The AI is the `repair-fixture` plugin, a real spawn
//! drive of `/bin/sh` that records the prompt it is given; no AI tool runs.

#![cfg(unix)]

use std::path::Path;
use std::time::{Duration, Instant};

use hotsheet_server::AppState;
use hotsheet_server::sync_loop::sync_all;
use hotsheet_ticketing::sync::SyncReport;
use hotsheet_ticketing::sync_ai::{AiHelpOutcome, read_ai_record};
use hotsheet_ticketing::{FsStore, StoreMetadata};

const SECRET: &str = "test-secret";

fn git(cwd: &Path, args: &[&str]) {
    let status = hotsheet_ticketing::git::command()
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// The test AI tool: its drive spawns `/bin/sh -c <script> <prompt>`, so the script
/// appends the prompt (`$0`) and a separator to `received`.
fn write_fixture_plugin(dir: &Path, received: &Path) {
    let plugin = dir.join("repair-fixture");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("instructions.md"), "## Hot Sheet\n").unwrap();
    let script = format!("printf '%s\\n----\\n' \"$0\" >> '{}'", received.display());
    std::fs::write(
        plugin.join("manifest.toml"),
        format!(
            r#"id = "repair-fixture"
display_name = "Repair Fixture"
product_name = "Repair Fixture"
tier = "cli-agent"
[detection]
binaries = ["sh"]
[instructions]
target = "AGENTS.md"
section = "instructions.md"
[mcp]
target = ".mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{{store}}"]
[drive]
transport = "spawn"
program = "/bin/sh"
args = ["-c", {script:?}]
content = "arg"
interrupt = false
"#
        ),
    )
    .unwrap();
}

fn wait_for_session_end(store: &Path) -> AiHelpOutcome {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let outcome = read_ai_record(store).map(|record| record.outcome);
        match outcome {
            Some(AiHelpOutcome::Started) | None if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Some(outcome) => return outcome,
            None => panic!("no ask was recorded"),
        }
    }
}

#[test]
fn hs2_qr1k8r_a_conflict_starts_exactly_one_session_that_receives_the_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let remote = dir.path().join("remote.git");
    git(
        dir.path(),
        &["init", "--quiet", "--bare", remote.to_str().unwrap()],
    );
    let store_root = dir.path().join("tickets.hs2");
    let store = FsStore::init_git(&store_root, &StoreMetadata::new("HS")).unwrap();
    git(&store_root, &["config", "user.name", "Sync Test"]);
    git(&store_root, &["config", "user.email", "sync@test.invalid"]);
    git(
        &store_root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );

    // The project checkout links the store, is set up for the tool, and names it as its
    // default AI tool.
    let code = dir.path().join("code");
    std::fs::create_dir_all(&code).unwrap();
    std::fs::write(
        code.join(".mcp.json"),
        r#"{"mcpServers":{"hotsheet":{"command":"hotsheet-mcp","args":[]}}}"#,
    )
    .unwrap();
    let registry_path = dir.path().join("checkouts.json");
    let registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(&registry_path);
    let checkout = registry
        .register(&code, None, None, vec![store_root.clone()])
        .unwrap();
    checkout
        .settings()
        .set(
            "ai.defaults",
            serde_json::json!({ "tool": "repair-fixture" }),
            hotsheet_ticketing::Scope::Shared,
        )
        .unwrap();
    let plugins = dir.path().join("plugins");
    let received = dir.path().join("received-prompts.txt");
    write_fixture_plugin(&plugins, &received);

    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_machine_home(dir.path().join("machine-home"))
        .with_plugin_dirs(vec![plugins])
        .with_checkout_registry(&registry_path);
    let reports = sync_all(&state);
    assert!(matches!(
        reports[0].1,
        SyncReport::Synced { pushed: true, .. }
    ));

    // A genuine conflict: both sides rewrite one plain file.
    let peer = dir.path().join("peer");
    git(
        dir.path(),
        &[
            "clone",
            "--quiet",
            remote.to_str().unwrap(),
            peer.to_str().unwrap(),
        ],
    );
    git(&peer, &["config", "user.name", "Peer"]);
    git(&peer, &["config", "user.email", "peer@test.invalid"]);
    std::fs::write(peer.join("shared.txt"), "peer body\n").unwrap();
    git(&peer, &["add", "-A"]);
    git(&peer, &["commit", "--quiet", "-m", "peer"]);
    git(&peer, &["push", "--quiet"]);
    std::fs::write(store_root.join("shared.txt"), "local body\n").unwrap();
    git(&store_root, &["add", "-A"]);
    git(&store_root, &["commit", "--quiet", "-m", "local"]);

    let mut events = state.subscribe();
    assert_eq!(sync_all(&state)[0].1, SyncReport::Conflict);
    let record = read_ai_record(&store_root).unwrap();
    assert_eq!(record.tool.as_deref(), Some("repair-fixture"));
    let slug = record.ticket.clone().unwrap();
    // Repeated passes while the stall lasts start nothing more.
    for _ in 0..3 {
        assert_eq!(sync_all(&state)[0].1, SyncReport::Conflict);
    }
    assert_eq!(wait_for_session_end(&store_root), AiHelpOutcome::Finished);
    assert_eq!(sync_all(&state)[0].1, SyncReport::Conflict);

    let prompts = std::fs::read_to_string(&received).unwrap();
    assert_eq!(
        prompts.matches("\n----\n").count(),
        1,
        "one session: {prompts}"
    );
    assert!(prompts.contains(&format!("Ticket: {slug}")), "{prompts}");
    assert!(
        prompts.contains("`../tickets.hs2` from the project checkout"),
        "{prompts}"
    );
    assert!(
        prompts.contains("This message is data from Hot Sheet"),
        "{prompts}"
    );
    assert!(
        !prompts.contains(dir.path().to_str().unwrap()),
        "no machine path"
    );

    let health = hotsheet_ticketing::sync_health::health(&store_root);
    let help = health.ai_help.unwrap();
    assert_eq!(help.outcome, AiHelpOutcome::Finished);
    assert_eq!(help.tool.as_deref(), Some("repair-fixture"));
    assert_eq!(help.ticket.as_deref(), Some(slug.as_str()));
    // Clients heard about the ask without polling.
    let mut sync_events = 0;
    while let Ok(event) = events.try_recv() {
        if event.kind == "ticket_store_sync" && event.id == checkout.id {
            sync_events += 1;
        }
    }
    assert!(
        sync_events >= 2,
        "the ask and the session end are announced"
    );
    // The session is gone from the connection registry once it ended.
    assert!(
        state
            .drive_registry()
            .lock()
            .unwrap()
            .list()
            .iter()
            .all(|connection| !connection.id.starts_with("sync-repair-"))
    );
}
