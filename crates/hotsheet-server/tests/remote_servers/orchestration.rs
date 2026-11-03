//! HS2-4J6V46: actual sibling CLI, authenticated service and stdio tool compositions.
use super::*;
use hotsheet_cli::orchestration::OrchestrationRuntime;
use hotsheet_ticketing::{
    ProviderConfigRegistry, ProviderConnection,
    checkouts::{CheckoutRegistry, TicketSource},
};
use std::path::Path;

const TARGET: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAY";
fn configured_owner(
    source: &Path,
    home: &Path,
    project: &Path,
) -> hotsheet_ticketing::checkouts::Checkout {
    std::fs::create_dir_all(source).unwrap();
    std::fs::create_dir_all(project).unwrap();
    let providers = ProviderConfigRegistry::new(source.join("providers.json"));
    providers
        .save(&[ProviderConnection {
            id: "synthetic-provider".into(),
            provider: "github".into(),
            locator: "synthetic/repo".into(),
            name: None,
            default: false,
            disabled: false,
            settings: Value::Null,
        }])
        .unwrap();
    CheckoutRegistry::new(home.join("checkouts.json"))
        .register_registered_sources(
            &providers,
            project,
            None,
            None,
            vec![TicketSource {
                connection_id: "synthetic-provider".into(),
                provider: "github".into(),
                locator: "synthetic/repo".into(),
            }],
            None,
        )
        .unwrap()
}
fn shared_runtime(source: &Path, home: &Path, memory: &Arc<Memory>) -> OrchestrationRuntime {
    OrchestrationRuntime::new(
        home.into(),
        CheckoutRegistry::new(home.join("checkouts.json")),
        ProviderConfigRegistry::new(source.join("providers.json")),
        memory.clone(),
        Arc::new(MtlsRemoteTransport),
        identity(home).server_id,
    )
    .unwrap()
}
async fn api(
    router: axum::Router,
    method: &str,
    path: &str,
    body: Value,
    authorized: bool,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if authorized {
        builder = builder.header("x-hotsheet-secret", SECRET);
    }
    let response = router
        .oneshot(
            builder
                .body(if method == "GET" {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_4j6v46_real_tls_cli_inspect_explicit_bind_and_revoked_candidate() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::at(root.path().join("tls"));
    init_ca(&paths, &[]).unwrap();
    let device = issue_device(&paths, "synthetic-reader").unwrap();
    set_device_role(&paths, "synthetic-reader", DeviceRole::ReadOnly).unwrap();
    let peer_home = root.path().join("peer-home");
    let peer_source = root.path().join("peer-store");
    let peer_state = state(&peer_source, &peer_home);
    let peer_project = root.path().join("peer-project");
    std::fs::create_dir(&peer_project).unwrap();
    let peer_checkout = CheckoutRegistry::new(peer_home.join("checkouts.json"))
        .register(&peer_project, None, None, vec![peer_source])
        .unwrap();
    let expected = identity(&peer_home);
    let peer = serve(peer_state, &paths, None).await;
    let home = root.path().join("controller-home");
    std::fs::create_dir(&home).unwrap();
    let source = root.path().join("controller-config");
    let owner = configured_owner(&source, &home, &root.path().join("owner"));
    let input = home.join("supplied-credentials.json");
    write_credentials(&input, &credentials(&device));
    cli(
        &home,
        &[
            "register",
            "--id",
            ID,
            "--label",
            "Synthetic",
            "--origin",
            &peer.origin,
            "--expected-server-id",
            &expected.server_id,
            "--credentials-file",
            input.to_str().unwrap(),
        ],
    );
    let prefix = [
        "-C",
        source.to_str().unwrap(),
        "orchestration",
        "target",
        "--project",
        &owner.id,
    ];
    let inspected = cli_named(
        &home,
        &prefix,
        &["inspect", &peer_checkout.id, "--registration-id", ID],
    );
    assert_eq!(inspected["usable"], false);
    assert_eq!(inspected["stale"], false);
    assert_eq!(inspected["projection"]["sources"][0]["provider"], "git");
    assert!(!home.join("orchestration-bindings.json").exists());
    assert!(
        !Path::new(&owner.root)
            .join(".hotsheet2/orchestration.json")
            .exists()
    );
    let added = cli_named(
        &home,
        &prefix,
        &["add", "--target-id", TARGET, "--label", "Synthetic target"],
    );
    let destination = inspected["destination"].to_string();
    assert_eq!(
        cli_named(
            &home,
            &prefix,
            &[
                "bind",
                TARGET,
                "--expected-manifest-revision",
                added["revision"].as_str().unwrap(),
                "--expected-binding-revision",
                "0",
                "--destination",
                &destination
            ]
        )["usable"],
        true
    );
    assert_eq!(
        cli_named(&home, &prefix, &["resolve", TARGET])["usable"],
        true
    );
    cli(&home, &["revoke", ID, "--expected-revision", "1"]);
    let unavailable = cli_named(&home, &prefix, &["resolve", TARGET]);
    assert_eq!(unavailable["reason"], "revoked");
    assert_eq!(unavailable["usable"], false);
    let bytes = std::fs::read_to_string(home.join("orchestration-bindings.json")).unwrap();
    for forbidden in [
        SECRET,
        peer.origin.as_str(),
        "credential_ref",
        "ca_pem",
        "client_key_pem",
    ] {
        assert!(!bytes.contains(forbidden));
    }
    peer.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_4j6v46_authenticated_http_all_target_operations_and_orphan_cleanup_before_runtime() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("host-store");
    let memory = Arc::new(Memory::default());
    let host = state(&source, &home);
    let owner = configured_owner(&source, &home, &root.path().join("owner"));
    let cleanup = app(host.clone());
    let router = app(host.with_orchestration_runtime(shared_runtime(&source, &home, &memory)));
    let prefix = format!("/checkouts/{}/orchestration/targets", owner.id);
    let inspect_path = format!("/checkouts/{}/orchestration/inspect", owner.id);
    assert_eq!(
        api(
            router.clone(),
            "POST",
            &inspect_path,
            json!({"destination":{"kind":"local","checkout_id":owner.id}}),
            false
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let candidate = api(
        router.clone(),
        "POST",
        &inspect_path,
        json!({"destination":{"kind":"local","checkout_id":owner.id}}),
        true,
    )
    .await
    .1;
    assert_eq!(candidate["usable"], false);
    assert!(!home.join("orchestration-bindings.json").exists());
    let added = api(
        router.clone(),
        "POST",
        &prefix,
        json!({"target_id":TARGET,"label":"Synthetic","expected_revision":null}),
        true,
    )
    .await
    .1;
    assert_eq!(
        api(
            router.clone(),
            "POST",
            &prefix,
            json!({"target_id":TARGET,"label":"Synthetic","expected_revision":null}),
            true
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let target = format!("{prefix}/{TARGET}");
    let binding = format!("{target}/binding");
    let qualified = json!({"owner_checkout_id":owner.id,"target_id":TARGET});
    let renamed = api(
        router.clone(),
        "PATCH",
        &target,
        json!({"label":"Renamed","expected_revision":added["revision"],"expected_repository_identity":"https://repo.example.test/group/project"}),
        true,
    )
    .await
    .1;
    assert_eq!(renamed["targets"][0]["target_id"], TARGET);
    assert_eq!(renamed["targets"][0]["label"], "Renamed");
    assert_eq!(
        renamed["targets"][0]["expected_repository_identity"],
        "https://repo.example.test/group/project"
    );
    assert_ne!(renamed["revision"], added["revision"]);
    assert_eq!(
        api(router.clone(), "GET", &prefix, Value::Null, true)
            .await
            .1,
        renamed
    );
    let manifest = Path::new(&owner.root).join(".hotsheet2/orchestration.json");
    let before = std::fs::read(&manifest).unwrap();
    assert_eq!(api(router.clone(), "PATCH", &target, json!({"label":"Wrong","expected_revision":renamed["revision"],"expected_repository_identity":"https://repo.example.test/other","clear_expected_repository_identity":true}), true).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(std::fs::read(&manifest).unwrap(), before);
    // Direct HTTP typed decoding remains strict for malformed explicit target IDs.
    for bad in [json!(123), json!(false), json!({})] {
        assert_eq!(
            api(
                router.clone(),
                "POST",
                &prefix,
                json!({"target_id":bad,"label":"Wrong","expected_revision":renamed["revision"]}),
                true
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(std::fs::read(&manifest).unwrap(), before);
    }
    let cleared = api(router.clone(), "PATCH", &target, json!({"label":"Renamed","expected_revision":renamed["revision"],"clear_expected_repository_identity":true}), true).await.1;
    assert_eq!(cleared["targets"][0]["target_id"], TARGET);
    assert_eq!(cleared["targets"][0]["label"], "Renamed");
    assert!(
        cleared["targets"][0]
            .get("expected_repository_identity")
            .is_none()
    );
    assert_ne!(cleared["revision"], renamed["revision"]);
    assert_eq!(
        api(router.clone(), "GET", &prefix, Value::Null, true)
            .await
            .1,
        cleared
    );
    let before = std::fs::read(&manifest).unwrap();
    assert_eq!(
        api(
            router.clone(),
            "PATCH",
            &target,
            json!({"label":"Stale","expected_revision":renamed["revision"]}),
            true
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(std::fs::read(&manifest).unwrap(), before);
    assert_eq!(api(router.clone(), "PATCH", &target, json!({"label":"Renamed","expected_revision":cleared["revision"],"clear_expected_repository_identity":true}), true).await.1, cleared);
    let after_clear = api(
        router.clone(),
        "PATCH",
        &target,
        json!({"label":"After clear","expected_revision":cleared["revision"]}),
        true,
    )
    .await
    .1;
    assert_eq!(after_clear["targets"][0]["target_id"], TARGET);
    assert_eq!(after_clear["targets"][0]["label"], "After clear");
    assert!(
        after_clear["targets"][0]
            .get("expected_repository_identity")
            .is_none()
    );
    assert_ne!(after_clear["revision"], cleared["revision"]);
    let cleared = after_clear;
    assert!(!home.join("orchestration-bindings.json").exists());
    let bound=api(router.clone(),"PUT",&binding,json!({"target":qualified,"expected_manifest_revision":cleared["revision"],"expected_binding_revision":0,"previous_destination":null,"destination":candidate["destination"]}),true).await;
    assert_eq!(bound.0, StatusCode::OK);
    assert_eq!(bound.1["usable"], true);
    assert_eq!(
        api(
            router.clone(),
            "GET",
            &format!("{target}/resolve"),
            Value::Null,
            true
        )
        .await
        .1["usable"],
        true
    );
    assert_eq!(
        api(router.clone(), "GET", &prefix, Value::Null, true)
            .await
            .1["targets"][0]["target_id"],
        TARGET
    );
    let projection = api(
        router.clone(),
        "GET",
        &format!("/remote-checkouts/{}/resolution", owner.id),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(projection.0, StatusCode::OK);
    assert_eq!(projection.1["sources"][0]["provider"], "github");
    assert!(
        serde_json::to_vec(&projection.1).unwrap().len()
            <= hotsheet_ticketing::orchestration::MAX_PROJECTION_BYTES
    );
    assert_eq!(
        api(
            router.clone(),
            "GET",
            "/remote-checkouts",
            Value::Null,
            true
        )
        .await
        .1[0]["resolution_metadata_version"],
        1
    );
    assert_eq!(
        api(
            router.clone(),
            "DELETE",
            &target,
            json!({"expected_revision":cleared["revision"]}),
            true
        )
        .await
        .1["targets"][0]["state"],
        "removed"
    );
    // The already-serving router without injected runtime must avoid every lazy owner.
    std::fs::write(home.join("checkouts.json"), b"corrupt").unwrap();
    std::fs::write(source.join("providers.json"), b"corrupt").unwrap();
    std::fs::write(home.join("server-identity.json"), b"corrupt").unwrap();
    assert_eq!(
        api(cleanup.clone(), "GET", &binding, Value::Null, true)
            .await
            .1["revision"],
        1
    );
    assert_eq!(api(cleanup,"DELETE",&binding,json!({"target":qualified,"expected_binding_revision":1,"previous_destination":candidate["destination"]}),true).await.1["revision"],2);
    assert_eq!(
        std::fs::read(home.join("server-identity.json")).unwrap(),
        b"corrupt"
    );
    assert!(memory.0.lock_or_recover().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_4j6v46_actual_project_path_server_stdio_target_workflows_share_the_same_runtime() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = home.join("server-bootstrap.hs2");
    let host = state(&source, &home);
    let owner = configured_owner(&source, &home, &root.path().join("owner"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let instance = hotsheet_cli::external_launch::instance_path(&home, &source);
    std::fs::create_dir_all(instance.parent().unwrap()).unwrap();
    let record=json!({"pid":std::process::id(),"url":origin,"secret":SECRET,"store_path":source.canonicalize().unwrap()}).to_string();
    std::fs::write(&instance, &record).unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app(host))
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let mut last_candidate = Value::Null;
    let mut last_id = String::new();
    for (index, backend) in [
        vec!["--project".to_string(), owner.root.clone()],
        vec![
            "--server".into(),
            origin.clone(),
            "--secret".into(),
            SECRET.into(),
        ],
        vec!["--path".into(), source.to_string_lossy().into_owned()],
    ]
    .into_iter()
    .enumerate()
    {
        let backend = backend.iter().map(String::as_str).collect::<Vec<_>>();
        assert_eq!(
            hotsheet_cli::external_launch::discover_running_server(&source, &home)
                .unwrap()
                .url,
            origin
        );
        assert_eq!(std::fs::read_to_string(&instance).unwrap(), record);
        let invoke = |mut args: Value| {
            if index != 0 {
                args["checkout"] = owner.id.clone().into();
            }
            mcp_named(&home, &backend, "hotsheet_targets", args)
        };
        let reject = |mut args: Value| {
            if index != 0 {
                args["checkout"] = owner.id.clone().into();
            }
            mcp_rejected(&home, &backend, "hotsheet_targets", args);
        };
        let candidate = invoke(json!({"action":"inspect","destination_checkout":owner.id}));
        assert_eq!(candidate["usable"], false);
        let id = hotsheet_model::Ulid::from(100u128 + index as u128).to_string();
        let current = invoke(json!({"action":"list"}));
        let manifest = Path::new(&owner.root).join(".hotsheet2/orchestration.json");
        let original = std::fs::read(&manifest).ok();
        let bindings = std::fs::read(home.join("orchestration-bindings.json")).ok();
        for bad in [
            json!(123),
            Value::Null,
            json!(true),
            json!({"id":"synthetic"}),
        ] {
            for _ in 0..2 {
                reject(
                    json!({"action":"add","target_id":bad,"label":"Wrong","expected_revision":current["revision"]}),
                );
                reject(
                    json!({"action":"inspect","registration_id":bad,"destination_checkout":owner.id}),
                );
                assert_eq!(std::fs::read(&manifest).ok(), original);
                assert_eq!(
                    std::fs::read(home.join("orchestration-bindings.json")).ok(),
                    bindings
                );
            }
        }
        let added = invoke(
            json!({"action":"add","target_id":id,"label":"Synthetic","expected_revision":current["revision"]}),
        );
        let renamed = invoke(
            json!({"action":"rename","target_id":id,"label":"Renamed","expected_revision":added["revision"],"expected_repository_identity":"https://repo.example.test/group/project"}),
        );
        let selected = |view: &Value| {
            view["targets"]
                .as_array()
                .unwrap()
                .iter()
                .find(|target| target["target_id"] == id)
                .unwrap()
                .clone()
        };
        assert_eq!(selected(&renamed)["target_id"], id);
        assert_eq!(selected(&renamed)["label"], "Renamed");
        assert_eq!(
            selected(&renamed)["expected_repository_identity"],
            "https://repo.example.test/group/project"
        );
        assert_ne!(renamed["revision"], added["revision"]);
        assert_eq!(invoke(json!({"action":"list"})), renamed);
        let before = std::fs::read(&manifest).unwrap();
        reject(
            json!({"action":"rename","target_id":id,"label":"Wrong","expected_revision":renamed["revision"],"expected_repository_identity":"https://repo.example.test/other","clear_expected_repository_identity":true}),
        );
        assert_eq!(std::fs::read(&manifest).unwrap(), before);
        let cleared = invoke(
            json!({"action":"rename","target_id":id,"label":"Renamed","expected_revision":renamed["revision"],"clear_expected_repository_identity":true}),
        );
        assert_eq!(selected(&cleared)["target_id"], id);
        assert_eq!(selected(&cleared)["label"], "Renamed");
        assert!(
            selected(&cleared)
                .get("expected_repository_identity")
                .is_none()
        );
        assert_ne!(cleared["revision"], renamed["revision"]);
        assert_eq!(invoke(json!({"action":"list"})), cleared);
        let before = std::fs::read(&manifest).unwrap();
        reject(
            json!({"action":"rename","target_id":id,"label":"Stale","expected_revision":renamed["revision"]}),
        );
        assert_eq!(std::fs::read(&manifest).unwrap(), before);
        assert_eq!(
            invoke(
                json!({"action":"rename","target_id":id,"label":"Renamed","expected_revision":cleared["revision"],"clear_expected_repository_identity":true})
            ),
            cleared
        );
        let after_clear = invoke(
            json!({"action":"rename","target_id":id,"label":"After clear","expected_revision":cleared["revision"]}),
        );
        assert_eq!(selected(&after_clear)["target_id"], id);
        assert_eq!(selected(&after_clear)["label"], "After clear");
        assert!(
            selected(&after_clear)
                .get("expected_repository_identity")
                .is_none()
        );
        assert_ne!(after_clear["revision"], cleared["revision"]);
        let cleared = after_clear;
        assert_eq!(
            std::fs::read(home.join("orchestration-bindings.json")).ok(),
            bindings
        );
        assert_eq!(
            invoke(
                json!({"action":"bind","target_id":id,"expected_manifest_revision":cleared["revision"],"expected_binding_revision":0,"previous_destination":null,"destination":candidate["destination"]})
            )["usable"],
            true
        );
        assert_eq!(
            invoke(json!({"action":"resolve","target_id":id}))["usable"],
            true
        );
        let removed = invoke(
            json!({"action":"remove","target_id":id,"expected_revision":cleared["revision"]}),
        );
        assert!(
            removed["targets"]
                .as_array()
                .unwrap()
                .iter()
                .any(|target| target["target_id"] == id && target["state"] == "removed")
        );
        assert_eq!(
            invoke(
                json!({"action":"unbind","target_id":id,"expected_binding_revision":1,"previous_destination":candidate["destination"]})
            )["revision"],
            2
        );
        last_candidate = candidate;
        last_id = id;
    }
    // Explicit --server stdio cleanup remains usable after owner/config/identity loss.
    std::fs::write(home.join("checkouts.json"), b"corrupt").unwrap();
    std::fs::write(source.join("providers.json"), b"corrupt").unwrap();
    std::fs::write(home.join("server-identity.json"), b"corrupt").unwrap();
    let backend = ["--server", origin.as_str(), "--secret", SECRET];
    assert_eq!(
        mcp_named(
            &home,
            &backend,
            "hotsheet_targets",
            json!({"action":"list","checkout":owner.id,"target_id":last_id})
        )["revision"],
        2
    );
    assert_eq!(
        mcp_named(
            &home,
            &backend,
            "hotsheet_targets",
            json!({"action":"unbind","checkout":owner.id,"target_id":last_id,"expected_binding_revision":2,"previous_destination":null})
        )["revision"],
        2
    );
    assert_eq!(last_candidate["usable"], false);
    assert_eq!(std::fs::read_to_string(&instance).unwrap(), record);
    assert!(
        hotsheet_process_journal::ProcessJournal::at(&home)
            .list()
            .unwrap()
            .iter()
            .all(|record| record.purpose != hotsheet_process_journal::Purpose::CliSetup)
    );
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_4j6v46_real_remote_project_and_http_stdio_inspect_requires_explicit_bind() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::at(root.path().join("tls"));
    init_ca(&paths, &[]).unwrap();
    let device = issue_device(&paths, "synthetic-metadata-reader").unwrap();
    set_device_role(&paths, "synthetic-metadata-reader", DeviceRole::ReadOnly).unwrap();
    let peer_home = root.path().join("peer-home");
    let peer_source = root.path().join("peer-store");
    let peer_state = state(&peer_source, &peer_home);
    let peer_project = root.path().join("peer-project");
    std::fs::create_dir(&peer_project).unwrap();
    let peer_checkout = CheckoutRegistry::new(peer_home.join("checkouts.json"))
        .register(&peer_project, None, None, vec![peer_source])
        .unwrap();
    let peer_identity = identity(&peer_home);
    let peer = serve(peer_state, &paths, None).await;
    let home = root.path().join("home");
    let source = home.join("server-bootstrap.hs2");
    let memory = Arc::new(Memory::default());
    let host = state(&source, &home);
    let owner = configured_owner(&source, &home, &root.path().join("owner"));
    let remotes = runtime(&home, &memory);
    remotes
        .register(hotsheet_ticketing::remote_servers::RegisterRemote {
            id: ID.into(),
            label: "Synthetic peer".into(),
            origin: peer.origin.clone(),
            expected_server_id: peer_identity.server_id,
            credentials: credentials(&device),
        })
        .unwrap();
    let host = host.with_orchestration_runtime(shared_runtime(&source, &home, &memory));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let instance = hotsheet_cli::external_launch::instance_path(&home, &source);
    std::fs::create_dir_all(instance.parent().unwrap()).unwrap();
    let record=json!({"pid":std::process::id(),"url":origin,"secret":SECRET,"store_path":source.canonicalize().unwrap()}).to_string();
    std::fs::write(&instance, &record).unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app(host))
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let project_args = ["--project", owner.root.as_str()];
    let server_args = ["--server", origin.as_str(), "--secret", SECRET];
    assert_eq!(
        hotsheet_cli::external_launch::discover_running_server(&source, &home)
            .unwrap()
            .url,
        origin
    );
    let candidate = mcp_named(
        &home,
        &project_args,
        "hotsheet_targets",
        json!({"action":"inspect","registration_id":ID,"destination_checkout":peer_checkout.id}),
    );
    assert_eq!(candidate["usable"], false);
    assert_eq!(candidate["projection"]["sources"][0]["provider"], "git");
    assert!(!home.join("orchestration-bindings.json").exists());
    assert!(
        !Path::new(&owner.root)
            .join(".hotsheet2/orchestration.json")
            .exists()
    );
    let added = mcp_named(
        &home,
        &project_args,
        "hotsheet_targets",
        json!({"action":"add","target_id":TARGET,"label":"Synthetic remote","expected_revision":null}),
    );
    let bound = mcp_named(
        &home,
        &server_args,
        "hotsheet_targets",
        json!({"action":"bind","checkout":owner.id,"target_id":TARGET,"expected_manifest_revision":added["revision"],"expected_binding_revision":0,"previous_destination":null,"destination":candidate["destination"]}),
    );
    assert_eq!(bound["usable"], true);
    assert_eq!(
        mcp_named(
            &home,
            &project_args,
            "hotsheet_targets",
            json!({"action":"resolve","target_id":TARGET})
        )["usable"],
        true
    );
    remotes.revoke(ID, 1).unwrap();
    let unavailable = mcp_named(
        &home,
        &server_args,
        "hotsheet_targets",
        json!({"action":"resolve","checkout":owner.id,"target_id":TARGET}),
    );
    assert_eq!(unavailable["usable"], false);
    assert_eq!(unavailable["reason"], "revoked");
    assert!(unavailable["sources"].is_null());
    assert_eq!(std::fs::read_to_string(&instance).unwrap(), record);
    assert!(
        hotsheet_process_journal::ProcessJournal::at(&home)
            .list()
            .unwrap()
            .iter()
            .all(|record| record.purpose != hotsheet_process_journal::Purpose::CliSetup)
    );
    let _ = stop.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
    peer.stop().await;
}
