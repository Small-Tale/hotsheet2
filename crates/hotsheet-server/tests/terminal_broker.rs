//! Creation-kind regressions through actual HTTP servers and the detached broker protocol.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hotsheet_server::terminal_broker::TerminalBroker;
use hotsheet_server::{AppState, app};
use hotsheet_terminals::TerminalManager;
use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::{Value, json};
use tokio::net::{TcpListener, UnixListener};

const SECRET: &str = "terminal-kind-test";

struct TestServer {
    base: String,
    task: tokio::task::JoinHandle<()>,
}

impl TestServer {
    async fn start(store: &Path, plugins: &Path, socket: Option<&Path>) -> Self {
        let mut state = AppState::new(FsStore::open(store).unwrap(), SECRET.into())
            .unwrap()
            .with_plugin_dirs(vec![plugins.to_path_buf()]);
        if let Some(socket) = socket {
            state = state.with_terminal_broker_at(TerminalBroker::at(socket, "kind-project"));
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });
        Self { base, task }
    }

    async fn request(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let request = ureq::request(method, &format!("{}{path}", self.base))
            .set("x-hotsheet-secret", SECRET)
            .set("content-type", "application/json");
        tokio::task::spawn_blocking(move || {
            let response = match body {
                Some(body) => request.send_string(&body.to_string()),
                None => request.call(),
            };
            let response = match response {
                Ok(response) | Err(ureq::Error::Status(_, response)) => response,
                Err(error) => panic!("HTTP request failed: {error}"),
            };
            let status = response.status();
            let text = response.into_string().unwrap();
            let value = if text.is_empty() {
                Value::Null
            } else {
                serde_json::from_str(&text).unwrap()
            };
            (status, value)
        })
        .await
        .unwrap()
    }

    async fn open(&self, body: Value) -> Value {
        let (status, info) = self.request("POST", "/terminals", Some(body)).await;
        assert_eq!(status, 200, "{info}");
        info
    }

    async fn list(&self) -> Value {
        let (status, info) = self.request("GET", "/terminals", None).await;
        assert_eq!(status, 200, "{info}");
        info
    }

    async fn assert_output_kind(&self, id: &str, kind: &str, link: Option<&str>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let (status, value) = self.request("GET", &format!("/terminals/{id}"), None).await;
            assert_eq!(status, 200, "{value}");
            assert_eq!(value["kind"], kind, "{value}");
            if value["link"].as_str() == link {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "OSC state did not arrive: {value}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn fixture() -> (tempfile::TempDir, tempfile::TempDir) {
    let store = tempfile::tempdir().unwrap();
    FsStore::init(store.path(), &StoreMetadata::new("HS")).unwrap();
    let plugins = tempfile::tempdir().unwrap();
    let plugin = plugins.path().join("kind-agent");
    std::fs::create_dir(&plugin).unwrap();
    std::fs::write(plugin.join("instructions.md"), "Terminal kind test agent\n").unwrap();
    std::fs::write(
        plugin.join("manifest.toml"),
        r#"
id = "kind-agent"
display_name = "Kind Agent"
product_name = "Kind Agent"
tier = "cli-agent"
[instructions]
target = "AGENTS.md"
section = "instructions.md"
[mcp]
target = ".mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
[launch]
program = "/bin/cat"
args = []
"#,
    )
    .unwrap();
    (store, plugins)
}

#[tokio::test]
async fn terminal_creation_kind_is_stable_across_http_reattach_and_osc_output() {
    for broker_mode in [false, true] {
        let (store, plugins) = fixture();
        let sockets = tempfile::tempdir().unwrap();
        let socket = sockets.path().join("broker.sock");
        let broker = if broker_mode {
            let listener = UnixListener::bind(&socket).unwrap();
            Some(tokio::spawn(hotsheet_terminals::serve_broker(
                listener,
                "kind-project".into(),
                Arc::new(TerminalManager::new()),
            )))
        } else {
            None
        };
        let server = TestServer::start(
            store.path(),
            plugins.path(),
            broker_mode.then_some(socket.as_path()),
        )
        .await;
        assert_eq!(server.open(json!({"id":"default"})).await["kind"], "shell");
        assert_eq!(
            server
                .open(json!({"id":"shell","command":"/bin/cat"}))
                .await["kind"],
            "shell"
        );
        assert_eq!(
            server.open(json!({"id":"ai","connect":"kind-agent"})).await["kind"],
            "ai"
        );
        assert_eq!(
            server
                .open(json!({"id":"explicit-ai","command":"/bin/cat","connect":"kind-agent"}))
                .await["kind"],
            "ai"
        );

        let (status, _) = server
            .request(
                "POST",
                "/terminals",
                Some(json!({"id":"invalid","connect":"missing-agent"})),
            )
            .await;
        assert_eq!(status, 400);
        assert_eq!(server.list().await.as_array().unwrap().len(), 4);

        // Reattaching with the opposite launch kind preserves each live PTY's origin.
        assert_eq!(
            server
                .open(json!({"id":"shell","connect":"kind-agent"}))
                .await["kind"],
            "shell"
        );
        assert_eq!(
            server.open(json!({"id":"ai","command":"/bin/cat"})).await["kind"],
            "ai"
        );
        for (id, kind) in [("shell", "shell"), ("ai", "ai")] {
            let input = format!("/terminals/{id}/input");
            let (status, _) = server.request("POST", &input, Some(json!({"data":"\u{1b}]0;AI agent\u{7}\u{1b}]8;;https://ai.example/session\u{7}\n"}))).await;
            assert_eq!(status, 204);
            server
                .assert_output_kind(id, kind, Some("https://ai.example/session"))
                .await;
            let (status, _) = server
                .request(
                    "POST",
                    &input,
                    Some(json!({"data":"\u{1b}]0;Shell\u{7}\u{1b}]8;;\u{7}\n"})),
                )
                .await;
            assert_eq!(status, 204);
            server.assert_output_kind(id, kind, None).await;
        }
        let list = server.list().await;
        for (id, kind) in [
            ("default", "shell"),
            ("shell", "shell"),
            ("ai", "ai"),
            ("explicit-ai", "ai"),
        ] {
            assert_eq!(
                list.as_array()
                    .unwrap()
                    .iter()
                    .find(|info| info["id"] == id)
                    .unwrap()["kind"],
                kind
            );
            assert_eq!(
                server
                    .request("DELETE", &format!("/terminals/{id}"), None)
                    .await
                    .0,
                204
            );
        }
        if let Some(broker) = broker {
            broker.abort();
        }
    }
}

#[tokio::test]
async fn broker_preserves_creation_kind_when_the_http_server_restarts() {
    let (store, plugins) = fixture();
    let sockets = tempfile::tempdir().unwrap();
    let socket = sockets.path().join("broker.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let broker = tokio::spawn(hotsheet_terminals::serve_broker(
        listener,
        "kind-project".into(),
        Arc::new(TerminalManager::new()),
    ));
    let first = TestServer::start(store.path(), plugins.path(), Some(&socket)).await;
    first.open(json!({"id":"ai","connect":"kind-agent"})).await;
    first.open(json!({"id":"shell","command":"/bin/cat"})).await;
    drop(first);

    let second = TestServer::start(store.path(), plugins.path(), Some(&socket)).await;
    for (id, kind, connect) in [("ai", "ai", None), ("shell", "shell", Some("kind-agent"))] {
        let list = second.list().await;
        let info = list
            .as_array()
            .unwrap()
            .iter()
            .find(|info| info["id"] == id)
            .unwrap();
        assert_eq!(info["kind"], kind);
        assert_eq!(info["alive"], true);
        second.assert_output_kind(id, kind, None).await;
        assert_eq!(
            second
                .open(json!({"id":id,"command":"/bin/cat","connect":connect}))
                .await["kind"],
            kind
        );
        assert_eq!(
            second
                .request("DELETE", &format!("/terminals/{id}"), None)
                .await
                .0,
            204
        );
    }
    broker.abort();
}
