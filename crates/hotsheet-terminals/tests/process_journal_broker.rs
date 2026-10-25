#![cfg(unix)]

use hotsheet_process_journal::{ExitReason, ProcessJournal, Purpose};
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

#[tokio::test]
async fn hs2_gdcmxe_broker_pty_records_launcher_and_session() {
    let home = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = home.path().join("broker.sock");
    let broker = BrokerChild(
        Command::new(env!("CARGO_BIN_EXE_hotsheet-terminal-broker"))
            .arg(&socket)
            .arg("project-1")
            .env("HOTSHEET_PROCESS_JOURNAL_HOME", home.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !socket.exists() {
        assert!(Instant::now() < deadline, "broker did not bind");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut client = BrokerClient::connect(&socket).await.unwrap();
    let response = client
        .request(&BrokerRequest::Open {
            id: "session-1".into(),
            kind: TerminalKind::Ai,
            command: "true".into(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            replay_bytes: None,
            push: None,
        })
        .await
        .unwrap();
    assert!(matches!(response, BrokerResponse::Terminal { .. }));
    let journal = ProcessJournal::at(home.path());
    loop {
        let records = journal.list().unwrap();
        if let Some(record) = records.first() {
            assert_eq!(record.supervisor_pid, broker.0.id());
            assert_eq!(record.supervisor, "terminal-broker");
            assert_eq!(record.project_id.as_deref(), Some("project-1"));
            assert_eq!(record.session_id.as_deref(), Some("session-1"));
            assert_eq!(record.purpose, Purpose::TerminalAi);
            if record.exit_reason == Some(ExitReason::Completed) {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "broker PTY exit was not recorded"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
