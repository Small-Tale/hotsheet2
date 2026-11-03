//! HS2-MRR6Z0: Codex MCP presence attribution against a real server socket.
//!
//! Two Codex terminals in one project each run `hotsheet-mcp` under their own TUI (a TUI with
//! a `--config` override or `--no-daemon` hosts its own session), so each shim forwards its
//! own terminal identity. A third
//! session's shim runs under the shared `codex app-server` daemon, whose environment still
//! names the first tab because that tab started it. The shim's real presence code (host
//! classification, identity filtering, HTTP heartbeat) must attribute the first two to their
//! own terminals and the daemon-hosted one to none. Synthetic ids only.

use std::time::{Duration, Instant};

use hotsheet_mcp::presence::{
    self, HttpPresence, Presence, PresenceTarget, hosted_by_shared_host, identity_env,
};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::{Value, json};
use tokio::net::TcpListener;

const SECRET: &str = "synthetic-codex-attribution";
const TAB_A: &str = "codex-tab-a";
const TAB_B: &str = "codex-tab-b";

fn request(method: &str, url: &str, body: Option<Value>) -> Value {
    let request = ureq::request(method, url)
        .set("x-hotsheet-secret", SECRET)
        .set("content-type", "application/json");
    let response = match body {
        Some(body) => request.send_string(&body.to_string()).unwrap(),
        None => request.call().unwrap(),
    };
    serde_json::from_str(&response.into_string().unwrap()).unwrap()
}

/// `(TAB_A connected, TAB_B connected)` from `GET /terminals`.
fn connected(base: &str) -> (bool, bool) {
    let rows = request("GET", &format!("{base}/terminals"), None);
    let of = |id: &str| {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap()["mcp_connected"]
            .as_bool()
            .unwrap()
    };
    (of(TAB_A), of(TAB_B))
}

/// Wait (bounded) until the terminal list reaches `expected`; leases register asynchronously.
fn settle(base: &str, expected: (bool, bool)) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while connected(base) != expected {
        assert!(
            Instant::now() < deadline,
            "terminals never reached {expected:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The shared hosts the bundled Codex plugin declares.
fn codex_hosts() -> Vec<hotsheet_plugins::SharedMcpHost> {
    let hosts = hotsheet_plugins::builtin_plugins()
        .into_iter()
        .find(|plugin| plugin.id() == "codex")
        .unwrap()
        .manifest
        .mcp
        .shared_hosts;
    assert!(
        !hosts.is_empty(),
        "the Codex plugin declares its app-server daemon"
    );
    hosts
}

/// Start one shim's presence exactly as `hotsheet-mcp` does, from the environment it sees
/// and its ancestors' command lines.
fn shim(base: &str, terminal: &str, ancestors: &[&str], lease: &str) -> Presence {
    let env = [
        ("HOTSHEET_SERVER", base.to_owned()),
        ("HOTSHEET_SECRET", SECRET.to_owned()),
        ("HOTSHEET_TERMINAL_ID", terminal.to_owned()),
        ("HOTSHEET_AGENT", "codex".to_owned()),
    ];
    let ancestors: Vec<String> = ancestors.iter().map(|c| (*c).to_owned()).collect();
    let registry = (base.to_owned(), SECRET.to_owned());
    let target = PresenceTarget::resolve(
        None,
        None,
        identity_env(
            |key| {
                env.iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, v)| v.clone())
            },
            hosted_by_shared_host(&ancestors, &codex_hosts()),
        ),
        || Some(registry),
        std::process::id(),
        Some("/synthetic/shared-project".into()),
        lease.into(),
    )
    .unwrap();
    Presence::start(target, HttpPresence::default(), presence::HEARTBEAT)
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_mrr6z0_two_codex_tabs_and_a_daemon_hosted_shim_are_not_confused() {
    let directory = tempfile::tempdir().unwrap();
    let store = FsStore::init(directory.path(), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_default_codex_home(directory.path().join("no-codex-home"))
        .with_machine_home(directory.path().join("machine-home"));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });

    tokio::task::spawn_blocking(move || {
        for id in [TAB_A, TAB_B] {
            request(
                "POST",
                &format!("{base}/terminals"),
                Some(json!({"id": id, "command": "cat", "connect": "codex"})),
            );
        }
        assert_eq!(connected(&base), (false, false));

        // The daemon-hosted shim alone: it inherited TAB_A's identity, but connects only the
        // project, never TAB_A.
        let daemon = shim(
            &base,
            TAB_A,
            &["/u/.codex/packages/app-server-daemon/bin/codex app-server --listen unix:// --managed-daemon"],
            "daemon-hosted",
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while request("GET", &format!("{base}/mcp-presence"), None)["connected"] != true {
            assert!(Instant::now() < deadline, "daemon-hosted lease never registered");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(connected(&base), (false, false));

        // Each tab's own shim connects only its own terminal.
        let a = shim(
            &base,
            TAB_A,
            &["codex --model gpt-test --config model_reasoning_effort=\"medium\""],
            "tab-a",
        );
        settle(&base, (true, false));
        let b = shim(&base, TAB_B, &["codex --no-daemon"], "tab-b");
        settle(&base, (true, true));
        let leases = request("GET", &format!("{base}/mcp-presence"), None);
        let mut terminals: Vec<Option<String>> = leases["leases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|lease| lease["terminal_id"].as_str().map(str::to_owned))
            .collect();
        terminals.sort();
        assert_eq!(
            terminals,
            [None, Some(TAB_A.to_owned()), Some(TAB_B.to_owned())]
        );

        // Closing one tab's session disconnects only that tab.
        b.stop();
        settle(&base, (true, false));
        // With TAB_A's own shim gone, the daemon-hosted shim that still carries TAB_A's
        // inherited identity does not keep TAB_A connected.
        a.stop();
        settle(&base, (false, false));
        assert_eq!(
            request("GET", &format!("{base}/mcp-presence"), None)["connected"],
            true
        );
        daemon.stop();
        assert_eq!(
            request("GET", &format!("{base}/mcp-presence"), None)["connected"],
            false
        );
    })
    .await
    .unwrap();
}
