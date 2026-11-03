//! HS2-CY01TZ: a broker launched with a dead Mach bootstrap port reports it and refuses to
//! spawn terminals that would inherit it, while a healthy broker keeps working.
#![cfg(target_os = "macos")]

use hotsheet_terminals::bootstrap::{
    BootstrapHealth, MACH_SEND_INVALID_DEST, null_bootstrap_for_child,
};
use hotsheet_terminals::{BrokerClient, BrokerRequest, BrokerResponse, TerminalKind};
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct BrokerChild(Child);

impl Drop for BrokerChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn start_broker(dead_bootstrap: bool) -> (tempfile::TempDir, BrokerChild, BrokerClient) {
    let home = tempfile::tempdir_in("/tmp").unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = home.path().join("broker.sock");
    let mut command = Command::new(env!("CARGO_BIN_EXE_hotsheet-terminal-broker"));
    command
        .arg(&socket)
        .arg("project-1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if dead_bootstrap {
        null_bootstrap_for_child(&mut command);
    }
    let mut broker = BrokerChild(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(15);
    while !socket.exists() {
        assert!(
            broker.0.try_wait().unwrap().is_none(),
            "broker exited before binding"
        );
        assert!(Instant::now() < deadline, "broker did not bind");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let client = BrokerClient::connect(&socket).await.unwrap();
    (home, broker, client)
}

fn open(id: &str) -> BrokerRequest {
    BrokerRequest::Open {
        id: id.into(),
        kind: TerminalKind::Shell,
        command: "cat".into(),
        args: Vec::new(),
        cwd: None,
        env: Vec::new(),
        replay_bytes: None,
        push: None,
    }
}

#[tokio::test]
async fn hs2_cy01tz_dead_bootstrap_broker_reports_it_and_refuses_new_terminals() {
    let (_home, _broker, mut client) = start_broker(true).await;
    // The broker still serves its protocol: liveness is not namespace health.
    assert!(matches!(
        client.request(&BrokerRequest::Ping).await.unwrap(),
        BrokerResponse::Pong
    ));
    match client.request(&BrokerRequest::Health).await.unwrap() {
        BrokerResponse::Health { bootstrap } => assert_eq!(
            bootstrap,
            BootstrapHealth::Dead {
                code: MACH_SEND_INVALID_DEST
            }
        ),
        other => panic!("unexpected health reply: {other:?}"),
    }
    match client.request(&open("t1")).await.unwrap() {
        BrokerResponse::Err { message } => {
            assert!(message.contains("HS2-CY01TZ"), "{message}");
            assert!(message.contains("--kill-all-terminals"), "{message}");
        }
        other => panic!("a dead-namespace broker spawned a terminal: {other:?}"),
    }
    // Nothing was spawned, so the broker stays empty and can idle out.
    assert!(matches!(
        client.request(&BrokerRequest::List).await.unwrap(),
        BrokerResponse::List { terminals } if terminals.is_empty()
    ));
}

#[tokio::test]
async fn hs2_cy01tz_healthy_broker_reports_a_live_namespace_and_opens() {
    let (_home, _broker, mut client) = start_broker(false).await;
    match client.request(&BrokerRequest::Health).await.unwrap() {
        BrokerResponse::Health { bootstrap } => assert!(!bootstrap.is_dead(), "{bootstrap:?}"),
        other => panic!("unexpected health reply: {other:?}"),
    }
    assert!(matches!(
        client.request(&open("t1")).await.unwrap(),
        BrokerResponse::Terminal { .. }
    ));
    // Reattaching to the live terminal is answered without a new spawn.
    match client.request(&open("t1")).await.unwrap() {
        BrokerResponse::Terminal { info } => assert_eq!(info.spawned, Some(false)),
        other => panic!("unexpected reattach reply: {other:?}"),
    }
    let _ = client
        .request(&BrokerRequest::Kill { id: "t1".into() })
        .await;
}
