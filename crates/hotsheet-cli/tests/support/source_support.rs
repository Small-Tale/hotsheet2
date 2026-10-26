//! HS2-FG7ZMF: actual CLI metadata reads and foreign-source failures without resolution.

use crate::support::Fixture;
use hotsheet_cli::coordination::{Assignment, HandoffEvidence, Phase, ReturnRoute, Snapshot};
use hotsheet_model::Ulid;
use hotsheet_ticketing::{ProviderConfigRegistry, ProviderConnection, git_connection_id};
use serde_json::json;
use std::fs;
use std::net::TcpListener;

fn run(f: &Fixture, args: &[&str]) -> std::process::Output {
    let args = f.coord(args);
    f.command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .arg("--test-fixture-credentials")
        .env("HOTSHEET_HOME", f.root.path().join("home"))
        .output()
        .unwrap()
}

fn snapshot(f: &Fixture, args: &[&str]) -> Snapshot {
    let output = run(f, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn connections(endpoint: &str) -> Vec<ProviderConnection> {
    ["github", "jira", "gitlab"].into_iter().enumerate().map(|(i, provider)| ProviderConnection {
        id: format!("source-{i}"), provider:provider.into(), locator:"synthetic/app".into(),
        name:None, default:false, disabled:false,
        settings:json!({"api_base":endpoint,"base_url":endpoint,"email":"fixture@example.test","credential":{"secret":"unresolvable-test-reference"}}),
    }).collect()
}

#[test]
fn hs2_fg7zmf_actual_cli_snapshots_and_rejections_make_zero_provider_or_secret_requests() {
    let f = Fixture::new();
    let home = f.root.path().join("home");
    fs::create_dir(&home).unwrap();
    // Any accidental credential registry read fails; explicit synthetic activation also
    // prevents native backend access even if a future regression reaches that boundary.
    fs::write(home.join("keys.json"), "not a credential registry").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let config = ProviderConfigRegistry::new(f.store.root().join("providers.json"));
    let connections = connections(&format!("http://{}", listener.local_addr().unwrap()));
    config.save(&connections).unwrap();
    let first = snapshot(&f, &["setup"]);
    let state_path = f.project.join(".git/hotsheet-coordination/state.json");
    let bytes = fs::read(&state_path).unwrap();
    let key_bytes = fs::read(home.join("keys.json")).unwrap();
    assert_eq!(
        first.uncovered_sources,
        ["source-0", "source-1", "source-2"]
    );
    for i in 0..3 {
        let support = &first
            .source_support
            .iter()
            .find(|s| s.connection_id == format!("source-{i}"))
            .unwrap();
        assert!(!support.capabilities.as_ref().unwrap().watch);
        assert!(
            !support.claim_verification && !support.queue_observed && !support.event_subscription
        );
    }
    assert!(
        first
            .source_support
            .iter()
            .find(|s| s.connection_id == git_connection_id(&f.store))
            .unwrap()
            .event_subscription
    );
    assert_eq!(snapshot(&f, &["status"]).cursor, first.cursor);
    assert_eq!(fs::read(&state_path).unwrap(), bytes);
    for connection in ["source-0", "source-1", "source-2", "unknown-source"] {
        let output = run(
            &f,
            &[
                "assign",
                "--id",
                "integrator",
                "--session",
                "root",
                "--worker",
                "worker",
                "--worker-session",
                "generation",
                "--operation",
                "foreign",
                "--connection",
                connection,
                "--native-id",
                "42",
                "--slug",
                "synthetic-42",
                "--base",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ],
        );
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("unsupported"), "{error}");
        assert!(error.contains(connection), "{error}");
        if connection != "unknown-source" {
            assert!(error.contains("native_claims_unsupported"), "{error}");
        } else {
            assert!(
                error.contains("unknown to the configured source metadata"),
                "{error}"
            );
        }
        assert_eq!(fs::read(&state_path).unwrap(), bytes);
    }
    config.set_disabled("source-0", true).unwrap();
    let disabled = snapshot(&f, &["status"]);
    assert!(!disabled.uncovered_sources.contains(&"source-0".into()));
    assert_ne!(disabled.cursor, first.cursor);
    config.set_disabled("source-0", false).unwrap();
    assert_eq!(snapshot(&f, &["status"]).cursor, first.cursor);
    assert_eq!(fs::read(&state_path).unwrap(), bytes);
    assert_eq!(fs::read(home.join("keys.json")).unwrap(), key_bytes);
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn hs2_fg7zmf_preserved_foreign_records_cannot_replay_accept_handoff_recover_verify_or_finish() {
    let f = Fixture::new();
    ProviderConfigRegistry::new(f.store.root().join("providers.json"))
        .save(&connections("http://127.0.0.1:9"))
        .unwrap();
    let mut state = snapshot(
        &f,
        &[
            "register",
            "--role",
            "integrator",
            "--id",
            "integrator",
            "--session",
            "root",
            "--lease-seconds",
            "3600",
        ],
    )
    .state;
    let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    state.assignments.insert(
        "foreign".into(),
        Assignment {
            id: "foreign".into(),
            worker: "worker".into(),
            generation: "generation".into(),
            ticket: hotsheet_ticketing::TicketRef {
                connection_id: "source-0".into(),
                native_id: "42".into(),
            },
            slug: "synthetic-42".into(),
            base: sha.into(),
            phase: Phase::Handoff,
            commit: Some(sha.into()),
            handoff: Some(HandoffEvidence::Code { commit: sha.into() }),
            return_route: ReturnRoute::HumanRelay,
            recovery_note: None,
        },
    );
    let path = f.project.join(".git/hotsheet-coordination/state.json");
    fs::write(&path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    let note = Ulid::new().to_string();
    let cases = vec![
        vec![
            "assign",
            "--id",
            "integrator",
            "--session",
            "root",
            "--worker",
            "worker",
            "--worker-session",
            "generation",
            "--operation",
            "foreign",
            "--connection",
            "source-0",
            "--native-id",
            "42",
            "--slug",
            "synthetic-42",
            "--base",
            sha,
        ],
        vec![
            "worker",
            "--id",
            "worker",
            "--session",
            "generation",
            "--assignment",
            "foreign",
            "--phase",
            "accepted",
        ],
        vec![
            "worker",
            "--id",
            "worker",
            "--session",
            "generation",
            "--assignment",
            "foreign",
            "--phase",
            "handoff",
            "--commit",
            sha,
        ],
        vec![
            "recover",
            "--id",
            "integrator",
            "--session",
            "root",
            "--assignment",
            "foreign",
            "--evidence-note",
            &note,
            "--inspected",
        ],
        vec![
            "mark",
            "--id",
            "integrator",
            "--session",
            "root",
            "--assignment",
            "foreign",
            "--phase",
            "integrating",
        ],
        vec![
            "mark",
            "--id",
            "integrator",
            "--session",
            "root",
            "--assignment",
            "foreign",
            "--phase",
            "verifying",
        ],
        vec![
            "mark",
            "--id",
            "integrator",
            "--session",
            "root",
            "--assignment",
            "foreign",
            "--phase",
            "done",
        ],
        vec![
            "review-return",
            "--id",
            "integrator",
            "--session",
            "root",
            "--assignment",
            "foreign",
            "--evidence-note",
            &note,
            "--inspected",
        ],
    ];
    for args in cases {
        let output = run(&f, &args);
        assert!(!output.status.success(), "accepted {args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("unsupported"),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read(&path).unwrap(),
            before,
            "mutated state on {args:?}"
        );
    }
    // No external queue is silently empty, even with no authorized operation available.
    assert!(
        !run(
            &f,
            &[
                "finish",
                "--id",
                "integrator",
                "--session",
                "root",
                "--reason",
                "drained"
            ]
        )
        .status
        .success()
    );
    assert_eq!(fs::read(&path).unwrap(), before);
}
