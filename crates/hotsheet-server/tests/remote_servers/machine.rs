//! HS2-VQ92XZ: `hotsheet-cli remote machine <binding>` against a real paired server, over a
//! desktop session pairing and a device certificate: the machine's host health, live CPU and
//! capacity with no open project, the long polls, a remote with no project, a revoked binding
//! and an unreachable remote. Scripted capacity readings only.
use super::*;
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const MIB: u64 = 1024 * 1024;

/// A machine with 6000 MB free of 16 GB, 20% busy and plenty of disk.
struct Steady;
impl hotsheet_server::CapacitySource for Steady {
    fn machine(&mut self) -> hotsheet_server::MachineReading {
        hotsheet_server::MachineReading {
            memory_free_bytes: Some(6000 * MIB),
            memory_total_bytes: Some(16_384 * MIB),
            cpu_percent: Some(20.0),
        }
    }
    fn disk(&mut self, _: &Path) -> Option<(u64, u64)> {
        Some((80_000 * MIB, 500_000 * MIB))
    }
    fn resident_bytes(&mut self, _: &[u32]) -> HashMap<u32, u64> {
        HashMap::new()
    }
    fn tools(&mut self, _: &[PathBuf]) -> Option<Vec<(String, bool)>> {
        Some(Vec::new())
    }
}

/// Serve a peer, with one hosted project when `hosting`, and register it on `home` as `id`.
async fn paired(
    pairing: Pairing,
    root: &Path,
    home: &Path,
    id: &str,
    hosting: bool,
) -> (PairedPeer, String) {
    let peer_home = root.join("peer-home");
    let peer_source = root.join("peer-store");
    let peer_state = state(&peer_source, &peer_home).with_capacity_source(Box::new(Steady));
    if hosting {
        let project = root.join("peer-project");
        std::fs::create_dir(&project).unwrap();
        CheckoutRegistry::new(peer_home.join("checkouts.json"))
            .register(&project, None, None, vec![peer_source.clone()])
            .unwrap();
    }
    let expected = identity(&peer_home);
    let peer = PairedPeer::serve(pairing, peer_state, root).await;
    let input = home.join(format!("{id}-credentials.json"));
    write_credentials(&input, &peer.credentials);
    cli(
        home,
        &[
            "register",
            "--id",
            id,
            "--label",
            "Synthetic build host",
            "--origin",
            &peer.origin,
            "--expected-server-id",
            &expected.server_id,
            "--credentials-file",
            input.to_str().unwrap(),
        ],
    );
    std::fs::remove_file(&input).unwrap();
    (peer, expected.server_id)
}

fn keys(value: &Value) -> Vec<&str> {
    let mut keys: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys
}

/// No session, cookie, certificate or key of the pairing appears in what was printed.
fn credential_free(printed: &str, peer: &PairedPeer) {
    let session = peer.credentials.bearer_secret.expose();
    let session = session.strip_prefix("session:").unwrap_or(session);
    assert!(!printed.contains(session));
    if let Some(key) = &peer.credentials.client_key_pem {
        assert!(!printed.contains(key.expose().lines().nth(1).unwrap()));
    }
    assert!(!printed.contains("hs2_remote_session"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_vq92xz_reads_a_paired_remote_machine_with_no_open_project() {
    for pairing in [Pairing::Session, Pairing::Certificate] {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("controller-home");
        std::fs::create_dir(&home).unwrap();
        let (peer, server_id) = paired(pairing, root.path(), &home, ID, true).await;

        // No reading named: health, CPU and capacity, through the hosted project.
        let all = cli(&home, &["machine", ID]);
        credential_free(&all.to_string(), &peer);
        assert_eq!(
            keys(&all),
            [
                "binding_id",
                "capacity",
                "cpu",
                "health",
                "host_name",
                "label",
                "server_id"
            ],
            "{pairing:?}: {all}"
        );
        assert_eq!(all["binding_id"], ID);
        assert_eq!(all["server_id"], server_id);
        assert_eq!(all["label"], "Synthetic build host");
        assert!(all["health"].is_object(), "{all}");
        assert!(all["cpu"]["seq"].is_u64(), "{all}");
        let version = all["capacity"]["version"].as_str().unwrap().to_owned();
        assert_eq!(
            all["capacity"]["memory"]["free_mb"], 6000,
            "{pairing:?}: {}",
            all["capacity"]
        );

        // Only what is named, in a repeatable read.
        for _ in 0..2 {
            let health = cli(&home, &["machine", ID, "--health"]);
            assert_eq!(
                keys(&health),
                ["binding_id", "health", "host_name", "label", "server_id"]
            );
        }

        // The capacity long poll: the same version waits, then answers with that version.
        let waited = std::time::Instant::now();
        let same = cli(
            &home,
            &[
                "machine",
                ID,
                "--capacity",
                "--after",
                &version,
                "--wait-seconds",
                "1",
            ],
        );
        assert!(waited.elapsed() >= std::time::Duration::from_millis(900));
        assert_eq!(same["capacity"]["version"], version.as_str());
        assert!(same.get("health").is_none() && same.get("cpu").is_none());
        // A version the remote never gave answers at once with the current one.
        let changed = cli(
            &home,
            &[
                "machine",
                ID,
                "--capacity",
                "--after",
                "0000",
                "--wait-seconds",
                "8",
            ],
        );
        assert_eq!(changed["capacity"]["version"], version.as_str());

        // The live CPU long poll answers with a frame.
        let cpu = cli(
            &home,
            &[
                "machine",
                ID,
                "--cpu",
                "--after",
                all["cpu"]["seq"].to_string().as_str(),
                "--wait-seconds",
                "1",
            ],
        );
        assert!(cpu["cpu"]["status"].is_string(), "{cpu}");
        assert_eq!(keys(&cpu).len(), 5);

        // A revoked binding is refused before any read.
        let listed = cli(&home, &["list"]);
        let revision = listed[0]["revision"].to_string();
        cli(
            &home,
            &["revoke", ID, "--expected-revision", revision.as_str()],
        );
        let refused = cli_failure(&home, &["remote"], &["machine", ID, "--health"]);
        assert!(refused.contains("revoked"), "{refused}");
        credential_free(&refused, &peer);
        peer.stop().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_vq92xz_a_remote_with_no_project_or_no_answer() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("controller-home");
    std::fs::create_dir(&home).unwrap();
    let (peer, _) = paired(Pairing::Session, root.path(), &home, ID, false).await;

    // Capacity has no project to go through: left out, with why, unless named.
    let all = cli(&home, &["machine", ID]);
    assert_eq!(all["capacity_unavailable"], "no_available_checkout");
    assert!(all.get("capacity").is_none());
    assert!(all["health"].is_object() && all["cpu"].is_object());
    let named = cli_failure(&home, &["remote"], &["machine", ID, "--capacity"]);
    assert!(named.contains("no_available_checkout"), "{named}");

    // Arguments that name no single long poll are refused before any request.
    let ambiguous = cli_failure(
        &home,
        &["remote"],
        &["machine", ID, "--after", "3", "--wait-seconds", "1"],
    );
    assert!(
        ambiguous.contains("exactly one of cpu or capacity"),
        "{ambiguous}"
    );

    // An unknown binding, and a remote that stopped answering.
    let unknown = cli_failure(
        &home,
        &["remote"],
        &["machine", "01ARZ3NDEKTSV4RRFFQ69G5FZZ"],
    );
    assert!(unknown.contains("not found"), "{unknown}");
    peer.stop().await;
    let gone = cli_failure(&home, &["remote"], &["machine", ID, "--health"]);
    assert!(gone.contains("unreachable"), "{gone}");
}

/// HS2-SW03MK: the `hotsheet_remotes` MCP tool reads a paired remote's machine and this
/// machine's paired session there, and asks to renew it, as `remote machine`, `remote
/// session` and `remote renew` do: through the stdio MCP process against a served
/// controller (`POST /remotes/{id}/machine`, `GET …/session`, `POST …/renew`, all
/// loopback-only) and serverless in process, over a desktop session pairing and a device
/// certificate. Nothing printed holds a session, cookie, certificate or key.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_sw03mk_mcp_reads_a_remote_machine_and_its_paired_session() {
    for pairing in [Pairing::Session, Pairing::Certificate] {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("controller-home");
        std::fs::create_dir(&home).unwrap();
        let peer_home = root.path().join("peer-home");
        let peer_source = root.path().join("peer-store");
        let peer_state = state(&peer_source, &peer_home).with_capacity_source(Box::new(Steady));
        let project = root.path().join("peer-project");
        std::fs::create_dir(&project).unwrap();
        CheckoutRegistry::new(peer_home.join("checkouts.json"))
            .register(&project, None, None, vec![peer_source.clone()])
            .unwrap();
        let server_id = identity(&peer_home).server_id;
        let peer = PairedPeer::serve(pairing, peer_state, root.path()).await;
        // The controller's registry keeps the binding's credential in memory.
        let memory = Arc::new(Memory::default());
        runtime(&home, &memory)
            .register(hotsheet_ticketing::remote_servers::RegisterRemote {
                id: ID.into(),
                label: "Synthetic build host".into(),
                origin: peer.origin.clone(),
                expected_server_id: server_id.clone(),
                credentials: peer.credentials.clone(),
            })
            .unwrap();
        let controller = state(&root.path().join("controller-store"), &home)
            .with_remote_runtime(runtime(&home, &memory));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let serving = tokio::spawn(async move {
            axum::serve(listener, hotsheet_server::app(controller))
                .with_graceful_shutdown(async move {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        let served = ["--server", origin.as_str(), "--secret", SECRET];
        let call = |arguments: Value| {
            let answer = mcp(&home, &served, arguments);
            credential_free(&answer.to_string(), &peer);
            answer
        };

        // No reading named: health, CPU and capacity, as the CLI prints them.
        let all = call(json!({"action":"machine","id":ID}));
        assert_eq!(
            keys(&all),
            [
                "binding_id",
                "capacity",
                "cpu",
                "health",
                "host_name",
                "label",
                "server_id"
            ],
            "{pairing:?}: {all}"
        );
        assert_eq!(all["server_id"], server_id.as_str());
        assert_eq!(all["capacity"]["memory"]["free_mb"], 6000, "{all}");
        let version = all["capacity"]["version"].as_str().unwrap().to_owned();
        // Only what is named, repeatably.
        for _ in 0..2 {
            let health = call(json!({"action":"machine","id":ID,"health":true}));
            assert_eq!(
                keys(&health),
                ["binding_id", "health", "host_name", "label", "server_id"]
            );
        }
        // The long polls: the capacity version it has waits, then answers with it; a CPU
        // frame seq is a number.
        let waited = std::time::Instant::now();
        let same = call(json!({"action":"machine","id":ID,"capacity":true,
            "after":version,"wait_ms":1000}));
        assert!(waited.elapsed() >= std::time::Duration::from_millis(900));
        assert_eq!(same["capacity"]["version"], version.as_str());
        let cpu = call(json!({"action":"machine","id":ID,"cpu":true,
            "after":all["cpu"]["seq"],"wait_ms":1000}));
        assert!(cpu["cpu"]["status"].is_string(), "{cpu}");
        // A cursor that names no single long poll is refused before any request.
        super::support::mcp_rejected(
            &home,
            &served,
            "hotsheet_remotes",
            json!({"action":"machine","id":ID,"after":3,"wait_ms":1000}),
        );

        // The paired session: its lifetime over a session pairing; a certificate binding
        // has none to read or renew.
        let session = call(json!({"action":"session","id":ID}));
        let renewed = call(json!({"action":"renew","id":ID}));
        assert_eq!(session["binding_id"], ID);
        match pairing {
            Pairing::Session => {
                assert!(session["device"]["expires_at"].is_number(), "{session}");
                assert!(session.get("failure").is_none(), "{session}");
                assert!(
                    renewed["device"]["renewal_requested_at"].is_number(),
                    "{renewed}"
                );
                // Asking again changes nothing a person has not approved yet.
                let again = call(json!({"action":"renew","id":ID}));
                assert!(again["device"]["renewal_requested_at"].is_number());
            }
            Pairing::Certificate => {
                assert_eq!(session["failure"], "unsupported", "{session}");
                assert_eq!(renewed["failure"], "unsupported", "{renewed}");
                assert!(session.get("device").is_none());
            }
        }

        // Serverless MCP runs the same workflow in process.
        let serverless = hotsheet_mcp::CoreBackend::new(
            FsStore::open(root.path().join("controller-store")).unwrap(),
        )
        .with_checkout_registry(CheckoutRegistry::new(home.join("checkouts.json")))
        .with_remote_runtime(runtime(&home, &memory));
        let local = |arguments: Value| {
            let reply = hotsheet_mcp::handle_message(
                &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                    "params":{"name":"hotsheet_remotes","arguments":arguments}}),
                &serverless,
            )
            .unwrap();
            credential_free(&reply.to_string(), &peer);
            reply
        };
        let text = |reply: &Value| -> Value {
            assert_eq!(reply["result"]["isError"], false, "{reply}");
            serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
        };
        let health = text(&local(json!({"action":"machine","id":ID,"health":true})));
        assert!(health["health"].is_object(), "{health}");
        let local_session = text(&local(json!({"action":"session","id":ID})));
        assert_eq!(local_session["binding_id"], ID);
        assert_eq!(
            local_session.get("failure").is_some(),
            matches!(pairing, Pairing::Certificate)
        );

        // A revoked binding is refused before any read, served or serverless.
        let revision = runtime(&home, &memory).list().unwrap()[0].revision;
        runtime(&home, &memory).revoke(ID, revision).unwrap();
        let refused = local(json!({"action":"machine","id":ID,"health":true}));
        assert_eq!(refused["result"]["isError"], true, "{refused}");
        assert!(refused.to_string().contains("revoked"), "{refused}");
        super::support::mcp_rejected(
            &home,
            &served,
            "hotsheet_remotes",
            json!({"action":"session","id":ID}),
        );
        let _ = stop.send(());
        serving.await.unwrap();
        peer.stop().await;
    }
}
