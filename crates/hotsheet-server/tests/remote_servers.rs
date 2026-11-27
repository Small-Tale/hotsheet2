//! HS2-1Y45MZ: isolated real TLS, CLI and service/MCP flows.
#[path = "remote_servers/support.rs"]
mod support;
// HS2-6EW369: the portable fixture program that validation steps run.
#[path = "../../test-support/fixture_program.rs"]
mod fixture_program;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use hotsheet_cli::remote_servers::MtlsRemoteTransport;
use hotsheet_server::app;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::FsStore;
use hotsheet_ticketing::remote_servers::RemoteTransport;
use hotsheet_tls::{DeviceRole, Paths, init_ca, issue_device, set_device_role};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::Arc;
use support::*;
use tower::ServiceExt;
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_1y45mz_real_cli_reconnect_review_ca_rotation_and_revocation() {
    let root = tempfile::tempdir().unwrap();
    let server_home = root.path().join("server-home");
    let client_home = root.path().join("client-home");
    std::fs::create_dir_all(&client_home).unwrap();
    let paths = Paths::at(root.path().join("tls"));
    init_ca(&paths, &[]).unwrap();
    let device = issue_device(&paths, "synthetic-reader").unwrap();
    set_device_role(&paths, "synthetic-reader", DeviceRole::ReadOnly).unwrap();
    let expected = identity(&server_home);
    let server_state = state(&root.path().join("server-store"), &server_home);
    let project = root.path().join("synthetic-project");
    std::fs::create_dir(&project).unwrap();
    let checkout_registry =
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(server_home.join("checkouts.json"));
    let checkout = checkout_registry
        .register(
            &project,
            Some("Synthetic project"),
            None,
            vec![root.path().join("server-store")],
        )
        .unwrap();
    let server = serve(server_state, &paths, None).await;
    let origin = server.origin.clone();
    let input = client_home.join("supplied-credentials.json");
    write_credentials(&input, &credentials(&device));
    let input_name = input.to_str().unwrap();
    let registered = cli(
        &client_home,
        &[
            "register",
            "--id",
            ID,
            "--label",
            "Synthetic",
            "--origin",
            &origin,
            "--expected-server-id",
            &expected.server_id,
            "--credentials-file",
            input_name,
        ],
    );
    assert_eq!(registered["checkouts"][0]["id"], checkout.id);
    assert_eq!(registered["checkouts"][0]["state"], "available");
    // HS2-HGG426: the live discovery names the peer machine for its project tabs.
    assert!(
        registered["host_name"]
            .as_str()
            .is_some_and(|name| !name.trim().is_empty() && !name.ends_with(".local")),
        "live discovery should carry the peer host name: {registered}"
    );
    assert!(
        !serde_json::to_string(&registered)
            .unwrap()
            .contains(project.to_str().unwrap())
    );
    assert_eq!(cli(&client_home, &["list"])[0]["stale"], true);
    let repeated = cli(
        &client_home,
        &[
            "register",
            "--id",
            ID,
            "--label",
            "Synthetic",
            "--origin",
            &origin,
            "--expected-server-id",
            &expected.server_id,
            "--credentials-file",
            input_name,
        ],
    );
    assert_eq!(repeated["remote"]["revision"], 1);
    let address = server.stop().await;
    let offline = cli(&client_home, &["checkouts", ID]);
    assert!(offline["checkouts"].is_null());
    assert_eq!(
        offline["remote"]["last_confirmed_checkouts"][0]["id"],
        checkout.id
    );
    assert_eq!(identity(&server_home), expected);
    // Same CA, new server leaf and same durable identity require no binding review.
    renew_server_leaf(&paths, vec!["localhost".into(), "127.0.0.1".into()]);
    let server = serve(
        state(&root.path().join("server-store"), &server_home),
        &paths,
        Some(address),
    )
    .await;
    assert!(cli(&client_home, &["checkouts", ID])["checkouts"].is_array());
    let address = server.stop().await;
    // A different home is a different application identity even with the same trusted CA.
    let changed_home = root.path().join("different-server-home");
    let changed_identity = identity(&changed_home);
    let server = serve(
        state(&root.path().join("server-store"), &changed_home),
        &paths,
        Some(address),
    )
    .await;
    let changed = cli(&client_home, &["checkouts", ID]);
    assert_eq!(changed["remote"]["last_failure"], "identity_changed");
    assert!(changed["checkouts"].is_null());
    let old_fingerprint = registered["remote"]["identity"]["ca_fingerprint"]
        .as_str()
        .unwrap();
    let reviewed = cli(
        &client_home,
        &[
            "review",
            ID,
            "--expected-revision",
            "1",
            "--previous-server-id",
            &expected.server_id,
            "--previous-ca-fingerprint",
            old_fingerprint,
            "--label",
            "Synthetic",
            "--origin",
            &origin,
            "--expected-server-id",
            &changed_identity.server_id,
            "--credentials-file",
            input_name,
        ],
    );
    assert_eq!(reviewed["remote"]["revision"], 2);
    let address = server.stop().await;
    // Changed trust cannot reconnect until explicitly reviewed with a new supplied CA/device.
    let other_paths = Paths::at(root.path().join("rotated-ca"));
    init_ca(&other_paths, &[]).unwrap();
    let other_device = issue_device(&other_paths, "synthetic-reader").unwrap();
    let server = serve(
        state(&root.path().join("server-store"), &changed_home),
        &other_paths,
        Some(address),
    )
    .await;
    assert!(cli(&client_home, &["checkouts", ID])["checkouts"].is_null());
    write_credentials(&input, &credentials(&other_device));
    let reviewed = cli(
        &client_home,
        &[
            "review",
            ID,
            "--expected-revision",
            "2",
            "--previous-server-id",
            &changed_identity.server_id,
            "--previous-ca-fingerprint",
            old_fingerprint,
            "--label",
            "Synthetic",
            "--origin",
            &origin,
            "--expected-server-id",
            &changed_identity.server_id,
            "--credentials-file",
            input_name,
        ],
    );
    assert_eq!(reviewed["remote"]["revision"], 3);
    assert_ne!(
        reviewed["remote"]["identity"]["ca_fingerprint"],
        registered["remote"]["identity"]["ca_fingerprint"]
    );
    hotsheet_tls::revoke_device(&other_paths, "synthetic-reader").unwrap();
    assert!(cli(&client_home, &["checkouts", ID])["checkouts"].is_null());
    let revoked = cli(&client_home, &["revoke", ID, "--expected-revision", "3"]);
    assert_eq!(revoked["revoked"], true);
    assert_eq!(
        cli(&client_home, &["revoke", ID, "--expected-revision", "3"])["revision"],
        4
    );
    let disk = std::fs::read_to_string(client_home.join("remotes.json")).unwrap();
    assert!(
        !disk.contains(SECRET)
            && !disk.contains("PRIVATE KEY")
            && !disk.contains("BEGIN CERTIFICATE")
    );
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_1y45mz_authenticated_service_and_serverless_mcp_use_the_same_registry() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::at(root.path().join("tls"));
    init_ca(&paths, &[]).unwrap();
    let device = issue_device(&paths, "synthetic-device").unwrap();
    let peer_home = root.path().join("peer-home");
    let peer_identity = identity(&peer_home);
    let server = serve(
        state(&root.path().join("peer-store"), &peer_home),
        &paths,
        None,
    )
    .await;
    let local_home = root.path().join("local-home");
    let memory = Arc::new(Memory::default());
    let local_state = state(&root.path().join("local-store"), &local_home)
        .with_remote_runtime(runtime(&local_home, &memory));
    let router = app(local_state.clone());
    let payload = json!({ "id": ID, "label": "Synthetic", "origin": server.origin, "expected_server_id": peer_identity.server_id, "credentials": credentials(&device) });
    let unauthorized = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/remotes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    let registered = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/remotes")
                .header("X-Hotsheet-Secret", SECRET)
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(registered.status(), StatusCode::OK);
    let body = registered.into_body().collect().await.unwrap().to_bytes();
    assert!(!String::from_utf8_lossy(&body).contains(SECRET));
    let registered: Value = serde_json::from_slice(&body).unwrap();
    let controller_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let controller_origin = format!("http://{}", controller_listener.local_addr().unwrap());
    let (controller_stop, controller_stopped) = tokio::sync::oneshot::channel();
    let controller = tokio::spawn(async move {
        axum::serve(controller_listener, app(local_state))
            .with_graceful_shutdown(async move {
                let _ = controller_stopped.await;
            })
            .await
            .unwrap();
    });
    let input = local_home.join("supplied-credentials.json");
    write_credentials(&input, &credentials(&device));
    let http_backend = hotsheet_mcp::HttpBackend::new(&controller_origin, SECRET);
    let reviewed = hotsheet_mcp::handle_message(&json!({ "jsonrpc":"2.0", "id":1, "method":"tools/call", "params":{
        "name":"hotsheet_remotes", "arguments":{ "action":"review", "id":ID, "expected_revision":1,
            "previous_identity":registered["remote"]["identity"], "label":"Synthetic", "origin":server.origin,
            "expected_server_id":peer_identity.server_id, "credentials_file":input } } }), &http_backend).unwrap();
    assert_eq!(reviewed["result"]["isError"], false);
    let store = FsStore::open(root.path().join("local-store")).unwrap();
    let backend = hotsheet_mcp::CoreBackend::new(store)
        .with_checkout_registry(hotsheet_ticketing::checkouts::CheckoutRegistry::new(
            local_home.join("checkouts.json"),
        ))
        .with_remote_runtime(runtime(&local_home, &memory));
    for action in ["list", "checkouts"] {
        let response = hotsheet_mcp::handle_message(
            &json!({ "jsonrpc":"2.0", "id":1, "method":"tools/call", "params":{
            "name":"hotsheet_remotes", "arguments":{"action":action,"id":ID} } }),
            &backend,
        )
        .unwrap();
        assert_eq!(response["result"]["isError"], false);
        let value: Value =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        if action == "list" {
            assert_eq!(value[0]["stale"], true);
        } else {
            assert!(value["checkouts"].is_array());
        }
    }
    let revoked = router
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/remotes/{ID}"))
                .header("X-Hotsheet-Secret", SECRET)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"expected_revision":2}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(revoked.status(), StatusCode::OK);
    assert!(memory.0.lock_or_recover().is_empty());
    let _ = controller_stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), controller)
        .await
        .unwrap()
        .unwrap();
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_w4jjje_mtls_uses_certificate_and_enforces_expiry_hostname_and_acl() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let paths = Paths::at(root.path().join("tls"));
    init_ca(&paths, &[]).unwrap();
    let device = issue_device(&paths, "synthetic-device").unwrap();
    let expected = identity(&home);
    let server = serve(state(&root.path().join("store"), &home), &paths, None).await;
    let origin = server.origin.clone();
    let good = credentials(&device);
    let mut wrong_bearer = good.clone();
    wrong_bearer.bearer_secret = "synthetic-wrong-bearer".into();
    // HS2-W4JJJE: the legacy machine bearer is not sent off-box. The valid
    // device certificate still authenticates this mTLS request.
    let probed = MtlsRemoteTransport
        .probe(&origin, &wrong_bearer)
        .unwrap()
        .identity;
    // The live identity adds the machine's display name (HS2-HGG426); the stored one never does.
    assert!(expected.host_name.is_none() && probed.host_name.is_some());
    assert_eq!(
        hotsheet_ticketing::remote_servers::RemoteIdentity {
            host_name: None,
            ..probed.clone()
        },
        expected
    );
    let expected = probed;
    let other = Paths::at(root.path().join("foreign"));
    init_ca(&other, &[]).unwrap();
    let foreign = credentials(&issue_device(&other, "synthetic-device").unwrap());
    assert!(MtlsRemoteTransport.probe(&origin, &foreign).is_err());
    let expired = expired_credentials(&paths);
    assert!(MtlsRemoteTransport.probe(&origin, &expired).is_err());
    set_device_role(&paths, "synthetic-device", DeviceRole::Deny).unwrap();
    assert_eq!(
        MtlsRemoteTransport.probe(&origin, &good).unwrap_err(),
        hotsheet_ticketing::remote_servers::RemoteFailure::Unauthorized
    );
    set_device_role(&paths, "synthetic-device", DeviceRole::ReadOnly).unwrap();
    assert_eq!(
        MtlsRemoteTransport.probe(&origin, &good).unwrap().identity,
        expected
    );
    let address = server.stop().await;
    renew_server_leaf(&paths, vec!["different.example.test".into()]);
    let server = serve(
        state(&root.path().join("store"), &home),
        &paths,
        Some(address),
    )
    .await;
    assert!(MtlsRemoteTransport.probe(&origin, &good).is_err());
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_1y45mz_shipped_project_path_and_server_mcp_stdio_share_remote_workflows() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::at(root.path().join("tls"));
    init_ca(&paths, &[]).unwrap();
    let device = issue_device(&paths, "synthetic-stdio-device").unwrap();
    let peer_home = root.path().join("peer-home");
    let peer_identity = identity(&peer_home);
    let peer = serve(
        state(&root.path().join("peer-store"), &peer_home),
        &paths,
        None,
    )
    .await;
    let home = root.path().join("controller-home");
    let bootstrap = home.join("server-bootstrap.hs2");
    let memory = Arc::new(Memory::default());
    let controller_state = state(&bootstrap, &home).with_remote_runtime(runtime(&home, &memory));
    let project = root.path().join("synthetic-project");
    std::fs::create_dir_all(&project).unwrap();
    let registry =
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json"));
    registry
        .register(
            &project,
            Some("Synthetic project"),
            None,
            vec![bootstrap.clone()],
        )
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let instance = hotsheet_cli::external_launch::instance_path(&home, &bootstrap);
    std::fs::create_dir_all(instance.parent().unwrap()).unwrap();
    let record = json!({"pid":std::process::id(), "url":origin, "secret":SECRET,
        "store_path":hotsheet_sync::paths::canonicalize(&bootstrap).unwrap()})
    .to_string();
    std::fs::write(&instance, &record).unwrap();
    assert_eq!(
        hotsheet_cli::external_launch::discover_running_server(&bootstrap, &home)
            .unwrap()
            .url,
        origin
    );
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let controller = tokio::spawn(async move {
        axum::serve(listener, app(controller_state))
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let input = home.join("supplied-credentials.json");
    write_credentials(&input, &credentials(&device));
    let project_args = ["--project", project.to_str().unwrap()];
    let registered = mcp(
        &home,
        &project_args,
        json!({"action":"register", "id":ID,
        "label":"Synthetic", "origin":peer.origin, "expected_server_id":peer_identity.server_id,
        "credentials_file":input}),
    );
    assert_eq!(registered["remote"]["revision"], 1);
    assert_eq!(
        mcp(&home, &project_args, json!({"action":"list"}))[0]["id"],
        ID
    );
    assert!(
        mcp(&home, &project_args, json!({"action":"checkouts", "id":ID}))["checkouts"].is_array()
    );
    let server_args = ["--server", &origin, "--secret", SECRET];
    assert_eq!(
        mcp(&home, &server_args, json!({"action":"list"}))[0]["id"],
        ID
    );
    assert!(
        mcp(&home, &server_args, json!({"action":"checkouts", "id":ID}))["checkouts"].is_array()
    );
    let path_args = ["--path", bootstrap.to_str().unwrap()];
    assert_eq!(
        mcp(&home, &path_args, json!({"action":"identity"}))["generation"],
        "hs2"
    );
    assert_eq!(
        mcp(&home, &path_args, json!({"action":"list"}))[0]["id"],
        ID
    );
    assert_eq!(
        mcp(
            &home,
            &project_args,
            json!({"action":"revoke", "id":ID, "expected_revision":1})
        )["revoked"],
        true
    );
    assert!(memory.0.lock_or_recover().is_empty());
    assert_eq!(std::fs::read_to_string(&instance).unwrap(), record);
    let records = hotsheet_process_journal::ProcessJournal::at(&home)
        .list()
        .unwrap();
    assert!(
        records
            .iter()
            .all(|record| record.purpose != hotsheet_process_journal::Purpose::CliSetup)
    );
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), controller)
        .await
        .unwrap()
        .unwrap();
    peer.stop().await;
}

// HS2-TZCCBJ: the automatic default-goal re-plan with a real PTY planner.
#[cfg(unix)]
#[path = "remote_servers/auto_replan.rs"]
mod auto_replan;
#[path = "remote_servers/capacity.rs"]
mod capacity;
#[path = "remote_servers/claim_provisional.rs"]
mod claim_provisional;
// HS2-2EY9DP: end-to-end fixtures and scenarios; real PTYs of the Unix fake agent.
#[cfg(unix)]
#[path = "remote_servers/e2e.rs"]
mod e2e;
#[cfg(unix)]
#[path = "remote_servers/e2e_coverage.rs"]
mod e2e_coverage;
#[cfg(unix)]
#[path = "remote_servers/e2e_failures.rs"]
mod e2e_failures;
#[cfg(unix)]
#[path = "remote_servers/e2e_loop.rs"]
mod e2e_loop;
// HS2-RCR4A6: one project on two servers, its sites sharing one store and one repository.
#[cfg(unix)]
#[path = "remote_servers/e2e_sites.rs"]
mod e2e_sites;
// HS2-1CC1TQ, HS2-5DF941: a multi-site project's home out of reach, on two servers.
#[cfg(unix)]
#[path = "remote_servers/e2e_home_outage.rs"]
mod e2e_home_outage;
// HS2-1NJ04Q: the scheduler's role sessions on a remote target, end to end.
#[cfg(unix)]
#[path = "remote_servers/e2e_roles.rs"]
mod e2e_roles;
// HS2-ZRPZJJ: a tool signed out on its machine is not placed until it signs in.
#[cfg(unix)]
#[path = "remote_servers/e2e_signed_out.rs"]
mod e2e_signed_out;
// HS2-P2TCQS: held work re-checks sign-in sooner than the capability detection.
#[cfg(unix)]
#[path = "remote_servers/e2e_sign_in_recheck.rs"]
mod e2e_sign_in_recheck;
// HS2-36XDDX: a template's Wait for Accept holds its steps until a person accepts.
#[cfg(unix)]
#[path = "remote_servers/e2e_templates.rs"]
mod e2e_templates;
// HS2-CC7TCB: a reported GitHub Actions run followed to its end by the Job monitor.
#[path = "remote_servers/ci_follow.rs"]
mod ci_follow;
#[path = "remote_servers/gates.rs"]
mod gates;
// HS2-92FJXM: a client on another machine adds the serving machine's open checkouts.
#[path = "remote_servers/hosted_projects.rs"]
mod hosted_projects;
// HS2-K2YE6A: the host watches only its checkout's own repository.
#[cfg(unix)]
#[path = "remote_servers/hosted_gates.rs"]
mod hosted_gates;
#[path = "remote_servers/job_monitor.rs"]
mod job_monitor;
#[path = "remote_servers/orchestration.rs"]
mod orchestration;
// HS2-HCW7MD: Propose Plan starts a real PTY of the Unix fake agent as the planner.
#[path = "remote_servers/permissions.rs"]
mod permissions;
#[cfg(unix)]
#[path = "remote_servers/planning.rs"]
mod planning;
// HS2-A4RVYA: applying a template starts the fake agent as the planner.
#[path = "remote_servers/policy.rs"]
mod policy;
#[path = "remote_servers/projects.rs"]
mod projects;
#[path = "remote_servers/provisional.rs"]
mod provisional;
#[path = "remote_servers/readiness.rs"]
mod readiness;
#[path = "remote_servers/role_sessions.rs"]
mod role_sessions;
#[cfg(unix)]
#[path = "remote_servers/templates.rs"]
mod templates;
// HS2-KXKA7X: scheduled and event triggers on a real server.
#[cfg(unix)]
#[path = "remote_servers/triggers.rs"]
mod triggers;
// HS2-75Q254: a real PTY session of the fake agent, a Unix test double.
#[cfg(unix)]
#[path = "remote_servers/reconcile_restart.rs"]
mod reconcile_restart;
#[path = "remote_servers/recovery.rs"]
mod recovery;
#[path = "remote_servers/remote_lines.rs"]
mod remote_lines;
#[path = "remote_servers/runs.rs"]
mod runs;
// HS2-GDJXGZ: real PTYs run the fake agent, which is a Unix test double.
#[path = "remote_servers/scheduler.rs"]
mod scheduler;
// HS2-95SWQ4: the scheduler places real PTY sessions of the same fake agent.
#[cfg(unix)]
#[path = "remote_servers/session_executor.rs"]
mod session_executor;
#[cfg(unix)]
#[path = "remote_servers/session_work.rs"]
mod session_work;
// HS2-DG0YPA, HS2-T16SK0, HS2-NPTKS8, HS2-R7QH35: the automatic loop's robustness,
// with real PTYs of the same fake agent.
#[cfg(unix)]
#[path = "remote_servers/session_loop.rs"]
mod session_loop;
#[path = "remote_servers/sessions.rs"]
mod sessions;
#[path = "remote_servers/summary.rs"]
mod summary;
#[path = "remote_servers/target_tickets.rs"]
mod target_tickets;
#[path = "remote_servers/timeline.rs"]
mod timeline;
#[path = "remote_servers/tool_capacity.rs"]
mod tool_capacity;
// HS2-5E0D96: runs the portable fixture program, so on every platform.
#[path = "remote_servers/validation.rs"]
mod validation;
