//! HS2-92FJXM: a client on another machine adds one of the serving machine's open
//! checkouts to an orchestration project over authenticated mTLS. A read-write device
//! may; a read-only device is refused by the write fence; a checkout the server does
//! not have open is refused; a repeat is the same project. Synthetic projects only.
use super::orchestration::{api, configured_owner, shared_runtime};
use super::*;
use hotsheet_cli::remote_servers::MtlsRemoteTransport;
use hotsheet_ticketing::remote_servers::{RemoteCheckoutTransport, RemoteCredentials};
use hotsheet_tls::{DeviceRole, init_ca, issue_device, set_device_role};

/// One request from the client device, as the desktop bridge forwards a remote tab's.
async fn call(
    origin: &str,
    credentials: &RemoteCredentials,
    method: &'static str,
    path: String,
    body: Option<Value>,
) -> (u16, Value) {
    let origin = origin.to_owned();
    let credentials = credentials.clone();
    tokio::task::spawn_blocking(move || {
        let bytes = body.map(|body| serde_json::to_vec(&body).unwrap());
        let reply = MtlsRemoteTransport
            .target_call(&origin, &credentials, method, &path, bytes.as_deref())
            .unwrap();
        (
            reply.status,
            serde_json::from_slice(&reply.body).unwrap_or(Value::Null),
        )
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_92fjxm_hs2_0g80wd_a_read_write_device_adds_and_removes_projects_over_mtls() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::at(root.path().join("tls"));
    init_ca(&paths, &[]).unwrap();
    let device = issue_device(&paths, "synthetic-client").unwrap();
    set_device_role(&paths, "synthetic-client", DeviceRole::ReadWrite).unwrap();
    let credentials = credentials(&device);
    // The serving machine: an orchestration project and two other open checkouts.
    let home = root.path().join("home");
    let source = root.path().join("host-store");
    let memory = Arc::new(Memory::default());
    let host = state(&source, &home);
    let owner = configured_owner(&source, &home, &root.path().join("owner"));
    let api_project = configured_owner(&source, &home, &root.path().join("api"));
    let docs_project = configured_owner(&source, &home, &root.path().join("docs"));
    let host = host.with_orchestration_runtime(shared_runtime(&source, &home, &memory));
    let router = app(host.clone());
    let base = format!("/checkouts/{}/orchestration", owner.id);
    let (status, set) = api(
        router.clone(),
        "PUT",
        &format!("{base}/project-type"),
        json!({"project_type":"orchestration","actor":{"role":"human"}}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    let served = serve(host.clone(), &paths, None).await;
    let hosted = "/orchestration/hosted-projects".to_owned();
    let person = json!({"role":"human","id":"remote-person"});

    // The list: this server's open checkouts, not the orchestration project itself.
    let (status, listed) = call(
        &served.origin,
        &credentials,
        "GET",
        format!("/checkouts/{}{hosted}", owner.id),
        None,
    )
    .await;
    assert_eq!(status, 200, "{listed}");
    let ids: Vec<_> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|choice| choice["checkout_id"].as_str().unwrap().to_owned())
        .collect();
    let mut expected = vec![api_project.id.clone(), docs_project.id.clone()];
    expected.sort();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(sorted, expected);
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .all(|choice| choice["added"] == false)
    );

    // A checkout the server does not have open is refused, and nothing is declared.
    let (status, refused) = call(
        &served.origin,
        &credentials,
        "POST",
        format!("/checkouts/{}{hosted}", owner.id),
        Some(json!({"checkout_id":"elsewhere-0123456789ab","actor":person})),
    )
    .await;
    assert_eq!(status, 403, "{refused}");
    assert_eq!(refused["code"], "forbidden", "{refused}");
    let (_, declared) = api(
        router.clone(),
        "GET",
        &format!("{base}/targets"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(declared["targets"], json!([]));

    // A read-write device adds one: declared and bound on the serving machine.
    let add = json!({"checkout_id":api_project.id,"label":"API","actor":person});
    let (status, added) = call(
        &served.origin,
        &credentials,
        "POST",
        format!("/checkouts/{}{hosted}", owner.id),
        Some(add.clone()),
    )
    .await;
    assert_eq!(status, 200, "{added}");
    assert_eq!(added["added"], true, "{added}");
    assert_eq!(added["resolution"]["usable"], true, "{added}");
    let target = added["target_id"].as_str().unwrap().to_owned();
    let (_, binding) = api(
        router.clone(),
        "GET",
        &format!("{base}/targets/{target}/binding"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(binding["destination"]["kind"], "local");
    assert_eq!(
        binding["destination"]["checkout_id"],
        api_project.id.as_str()
    );
    // Repeated: the same project, nothing declared twice.
    let (status, again) = call(
        &served.origin,
        &credentials,
        "POST",
        format!("/checkouts/{}{hosted}", owner.id),
        Some(add.clone()),
    )
    .await;
    assert_eq!(status, 200, "{again}");
    assert_eq!(again["added"], false, "{again}");
    assert_eq!(again["target_id"], target.as_str());
    let (_, declared) = api(
        router.clone(),
        "GET",
        &format!("{base}/targets"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(declared["targets"].as_array().unwrap().len(), 1);
    // The list now marks it added, with its target.
    let (_, listed) = call(
        &served.origin,
        &credentials,
        "GET",
        format!("/checkouts/{}{hosted}", owner.id),
        None,
    )
    .await;
    let api_choice = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|choice| choice["checkout_id"] == api_project.id.as_str())
        .unwrap()
        .clone();
    assert_eq!(api_choice["added"], true);
    assert_eq!(api_choice["target_id"], target.as_str());
    // Adding is a person's act: an AI actor is refused.
    let (status, ai) = call(
        &served.origin,
        &credentials,
        "POST",
        format!("/checkouts/{}{hosted}", owner.id),
        Some(json!({"checkout_id":docs_project.id,"actor":{"role":"ai","id":"worker-1"}})),
    )
    .await;
    assert_eq!(status, 403, "{ai}");

    // A read-only device may list but not add.
    set_device_role(&paths, "synthetic-client", DeviceRole::ReadOnly).unwrap();
    let (status, _) = call(
        &served.origin,
        &credentials,
        "GET",
        format!("/checkouts/{}{hosted}", owner.id),
        None,
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = call(
        &served.origin,
        &credentials,
        "POST",
        format!("/checkouts/{}{hosted}", owner.id),
        Some(json!({"checkout_id":docs_project.id,"actor":person})),
    )
    .await;
    assert_eq!(status, 403);
    let (_, declared) = api(
        router.clone(),
        "GET",
        &format!("{base}/targets"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(declared["targets"].as_array().unwrap().len(), 1);

    // Headless on the serving machine: the same route through the runtime.
    set_device_role(&paths, "synthetic-client", DeviceRole::ReadWrite).unwrap();
    let human = [
        "--actor-role",
        "human",
        "--actor-id",
        "maintainer",
        "-C",
        source.to_str().unwrap(),
        "orchestration",
    ];
    let choices = cli_named(
        &home,
        &human,
        &["project", "hosted", "--project", &owner.id],
    );
    assert_eq!(choices.as_array().unwrap().len(), 2);
    let docs = cli_named(
        &home,
        &human,
        &[
            "project",
            "add-hosted",
            &docs_project.id,
            "--label",
            "Docs",
            "--project",
            &owner.id,
        ],
    );
    assert_eq!(docs["added"], true, "{docs}");
    let unopened = cli_failure(
        &home,
        &human,
        &[
            "project",
            "add-hosted",
            "elsewhere-0123456789ab",
            "--project",
            &owner.id,
        ],
    );
    assert!(unopened.contains("forbidden"), "{unopened}");

    // HS2-0G80WD: removing mirrors adding. An AI actor is refused, and so is a
    // read-only device; nothing changes.
    let remove = |target: &str| format!("/checkouts/{}{hosted}/{target}", owner.id);
    let active = |declared: &Value| {
        declared["targets"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["state"] == "active")
            .count()
    };
    let (status, ai) = call(
        &served.origin,
        &credentials,
        "DELETE",
        remove(&target),
        Some(json!({"actor":{"role":"ai","id":"worker-1"}})),
    )
    .await;
    assert_eq!(status, 403, "{ai}");
    set_device_role(&paths, "synthetic-client", DeviceRole::ReadOnly).unwrap();
    let (status, _) = call(
        &served.origin,
        &credentials,
        "DELETE",
        remove(&target),
        Some(json!({"actor":person})),
    )
    .await;
    assert_eq!(status, 403);
    set_device_role(&paths, "synthetic-client", DeviceRole::ReadWrite).unwrap();
    let declared = api(
        router.clone(),
        "GET",
        &format!("{base}/targets"),
        Value::Null,
        true,
    )
    .await
    .1;
    assert_eq!(active(&declared), 2);
    // A read-write device removes it: unbound here and tombstoned.
    let (status, removed) = call(
        &served.origin,
        &credentials,
        "DELETE",
        remove(&target),
        Some(json!({"actor":person})),
    )
    .await;
    assert_eq!(status, 200, "{removed}");
    assert_eq!(
        (&removed["unbound"], &removed["removed"]),
        (&json!(true), &json!(true))
    );
    let (_, binding) = api(
        router.clone(),
        "GET",
        &format!("{base}/targets/{target}/binding"),
        Value::Null,
        true,
    )
    .await;
    assert!(binding["destination"].is_null(), "{binding}");
    // Repeated: nothing changes.
    let (status, again) = call(
        &served.origin,
        &credentials,
        "DELETE",
        remove(&target),
        Some(json!({"actor":person})),
    )
    .await;
    assert_eq!(status, 200, "{again}");
    assert_eq!(
        (&again["unbound"], &again["removed"]),
        (&json!(false), &json!(false))
    );
    // An unknown project is not found.
    let (status, unknown) = call(
        &served.origin,
        &credentials,
        "DELETE",
        remove("01ARZ3NDEKTSV4RRFFQ69G5FZZ"),
        Some(json!({"actor":person})),
    )
    .await;
    assert_eq!(status, 404, "{unknown}");
    assert_eq!(unknown["code"], "target_not_found", "{unknown}");
    // The list offers API again, and Docs is still added.
    let (_, listed) = call(
        &served.origin,
        &credentials,
        "GET",
        format!("/checkouts/{}{hosted}", owner.id),
        None,
    )
    .await;
    let added = |id: &str| {
        listed
            .as_array()
            .unwrap()
            .iter()
            .find(|choice| choice["checkout_id"] == id)
            .unwrap()["added"]
            .clone()
    };
    assert_eq!(added(&api_project.id), false);
    assert_eq!(added(&docs_project.id), true);
    // Headless, the same: an AI actor refused, then a person removes Docs.
    let docs_target = docs["target_id"].as_str().unwrap().to_owned();
    let ai_cli = [
        "--actor-role",
        "ai",
        "--actor-id",
        "worker-1",
        "-C",
        source.to_str().unwrap(),
        "orchestration",
    ];
    let refused = cli_failure(
        &home,
        &ai_cli,
        &[
            "project",
            "remove-hosted",
            &docs_target,
            "--project",
            &owner.id,
        ],
    );
    assert!(refused.contains("forbidden"), "{refused}");
    let gone = cli_named(
        &home,
        &human,
        &[
            "project",
            "remove-hosted",
            &docs_target,
            "--project",
            &owner.id,
        ],
    );
    assert_eq!(gone["removed"], true, "{gone}");
    served.stop().await;
}

/// HS2-89YNGN: a client on another machine adds one of the serving machine's open
/// checkouts as another site of a project, over authenticated mTLS and headless. Each
/// hosted choice names its ticket store, a site of an unknown project is refused before
/// anything is declared, and `project add-hosted --site-of` is the same workflow.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_89yngn_a_remote_client_adds_a_hosted_checkout_as_a_site() {
    assert!(hotsheet_aitools::test_permission_vault_available());
    hotsheet_aitools::enable_test_permission_vault();
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::at(root.path().join("tls"));
    init_ca(&paths, &[]).unwrap();
    let device = issue_device(&paths, "synthetic-client").unwrap();
    set_device_role(&paths, "synthetic-client", DeviceRole::ReadWrite).unwrap();
    let credentials = credentials(&device);
    let home = root.path().join("home");
    let source = root.path().join("host-store");
    let memory = Arc::new(Memory::default());
    let host = state(&source, &home);
    let owner = configured_owner(&source, &home, &root.path().join("owner"));
    let api_project = configured_owner(&source, &home, &root.path().join("api"));
    let docs_project = configured_owner(&source, &home, &root.path().join("docs"));
    let clone_project = configured_owner(&source, &home, &root.path().join("clone"));
    let host = host.with_orchestration_runtime(shared_runtime(&source, &home, &memory));
    let router = app(host.clone());
    let base = format!("/checkouts/{}/orchestration", owner.id);
    let (status, set) = api(
        router.clone(),
        "PUT",
        &format!("{base}/project-type"),
        json!({"project_type":"orchestration","actor":{"role":"human"}}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    let served = serve(host.clone(), &paths, None).await;
    let hosted = format!("/checkouts/{}/orchestration/hosted-projects", owner.id);
    let person = json!({"role":"human","id":"remote-person"});

    // Every choice names the store it reaches: these three share one.
    let (status, listed) = call(&served.origin, &credentials, "GET", hosted.clone(), None).await;
    assert_eq!(status, 200, "{listed}");
    let stores: Vec<&Value> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|choice| &choice["checkout_authority"])
        .collect();
    assert_eq!(stores.len(), 3, "{listed}");
    assert!(stores[0].is_string(), "{listed}");
    assert!(stores.iter().all(|store| *store == stores[0]), "{listed}");

    let (status, added) = call(
        &served.origin,
        &credentials,
        "POST",
        hosted.clone(),
        Some(json!({"checkout_id":api_project.id,"label":"API","actor":person})),
    )
    .await;
    assert_eq!(status, 200, "{added}");
    let project = added["target_id"].as_str().unwrap().to_owned();
    // A site of a project that is not declared is refused before anything is declared.
    let (status, unknown) = call(
        &served.origin,
        &credentials,
        "POST",
        hosted.clone(),
        Some(json!({"checkout_id":docs_project.id,"label":"Docs","site_of":"01ARZ3NDEKTSV4RRFFQ69G5FAV","actor":person})),
    )
    .await;
    assert_eq!(status, 404, "{unknown}");
    assert_eq!(unknown["code"], "site_project_unknown", "{unknown}");
    let declared = |router: axum::Router| {
        let base = base.clone();
        async move {
            api(router, "GET", &format!("{base}/targets"), Value::Null, true)
                .await
                .1["targets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|target| {
                    (
                        target["label"].as_str().unwrap().to_owned(),
                        target["site_of"].as_str().map(str::to_owned),
                    )
                })
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(
        declared(router.clone()).await,
        vec![("API".to_owned(), None)]
    );
    // Over mTLS: Docs is declared and bound as a site of API in one step.
    let (status, site) = call(
        &served.origin,
        &credentials,
        "POST",
        hosted.clone(),
        Some(
            json!({"checkout_id":docs_project.id,"label":"Docs","site_of":project,"actor":person}),
        ),
    )
    .await;
    assert_eq!(status, 200, "{site}");
    assert_eq!(site["added"], true, "{site}");
    // Headless on the serving machine: `project add-hosted --site-of`.
    let human = [
        "--actor-role",
        "human",
        "--actor-id",
        "maintainer",
        "-C",
        source.to_str().unwrap(),
        "orchestration",
    ];
    let clone = cli_named(
        &home,
        &human,
        &[
            "project",
            "add-hosted",
            &clone_project.id,
            "--label",
            "Clone",
            "--site-of",
            &project,
            "--project",
            &owner.id,
        ],
    );
    assert_eq!(clone["added"], true, "{clone}");
    assert_eq!(
        declared(router.clone()).await,
        vec![
            ("API".to_owned(), None),
            ("Docs".to_owned(), Some(project.clone())),
            ("Clone".to_owned(), Some(project.clone())),
        ]
    );
}
