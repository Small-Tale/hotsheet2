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
async fn hs2_92fjxm_a_read_write_device_adds_an_open_checkout_over_mtls() {
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
    served.stop().await;
}
