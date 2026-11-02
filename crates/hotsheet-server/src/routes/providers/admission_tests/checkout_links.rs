//! HS2-6H8ZY6 B3a: called by the existing private capability-checked child only.
use super::*;
use hotsheet_ticketing::checkouts::TicketSource;
use hotsheet_ticketing::github_endpoint::Expected;
use serde_json::{Value, json};

fn source(row: &ProviderConnection) -> TicketSource {
    TicketSource {
        connection_id: row.id.clone(),
        provider: row.provider.clone(),
        locator: row.locator.clone(),
    }
}
fn known(config: &ProviderConfigRegistry, id: &str, pending: bool) -> ProviderConnection {
    let row = ProviderConnection {
        id: id.into(),
        provider: "github".into(),
        locator: format!("synthetic/{id}"),
        name: None,
        default: false,
        disabled: false,
        settings: json!({"credential":{"secret":ACCOUNT}}),
    };
    EndpointRegistry::new(config)
        .apply(
            &Request {
                operation_id: ulid::Ulid::new(),
                expected: None,
                connection: row,
                intent: Intent::Explicit {
                    credential_ref: ACCOUNT.into(),
                    endpoint: Endpoint::Public,
                },
                link: pending.then(|| Link {
                    checkout_id: "b3-pending-fixture".into(),
                    make_default: false,
                }),
            },
            &Observation::Unavailable,
            &NoLinks,
        )
        .unwrap()
        .connection
}
fn bytes(state: &AppState, config: &ProviderConfigRegistry) -> (Vec<u8>, Vec<u8>) {
    (
        std::fs::read(state.checkout_registry.path()).unwrap(),
        std::fs::read(config.path()).unwrap(),
    )
}
fn cli(home: &Path, state: &AppState) -> Command {
    let binary = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!("hotsheet-cli{}", std::env::consts::EXE_SUFFIX));
    let mut command = hotsheet_ticketing::git::launch(binary);
    isolate(&mut command, home);
    command
        .args(["--test-fixture-credentials", "-C"])
        .arg(state.store.root());
    command
}

pub(super) async fn hs2_6h8zy6_actual_b3a_checkout_http_cli_parents(
    state: &AppState,
    config: &ProviderConfigRegistry,
    home: &Path,
) {
    assert!(hotsheet_ticketing::secrets::test_credentials_available());
    assert_eq!(
        std::env::var_os("HOTSHEET_HOME").as_deref(),
        Some(home.as_os_str())
    );
    let a = known(config, "b3-parent-a", false);
    let b = known(config, "b3-parent-b", false);
    let pending = known(config, "b3-parent-pending", true);
    let root = home.join("b3-parent-project");
    std::fs::create_dir(&root).unwrap();
    let checkout = state
        .checkout_registry
        .register_registered_sources(
            config,
            &root,
            Some("b3-parent"),
            None,
            vec![source(&a)],
            Some(a.id.clone()),
        )
        .unwrap();
    // A historical link can outlive a transition to pending. Seed that persisted
    // shape directly; public registration must reject new pending membership.
    let mut saved: Value =
        serde_json::from_slice(&std::fs::read(state.checkout_registry.path()).unwrap()).unwrap();
    let entry = saved["checkouts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["id"] == checkout.id)
        .unwrap();
    entry["sources"] = json!([source(&a), source(&pending)]);
    std::fs::write(
        state.checkout_registry.path(),
        serde_json::to_vec(&saved).unwrap(),
    )
    .unwrap();
    let add = format!("/checkouts/{}/sources/{}", checkout.id, pending.id);
    let defaults = format!("/checkouts/{}/default-source", checkout.id);
    let before = bytes(state, config);
    let (status, body) = request(
        state,
        "PUT",
        &add,
        json!({"provider":"github","locator":pending.locator}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], EndpointError::Pending.to_string());
    assert_eq!(bytes(state, config), before);
    let (status, _) = request(
        state,
        "PUT",
        &add,
        json!({"provider":"jira","locator":pending.locator}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "structural provider collision precedes pending"
    );
    assert_eq!(bytes(state, config), before);
    let (status, body) =
        request(state, "PUT", &defaults, json!({"connection_id":pending.id})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], EndpointError::Pending.to_string());
    assert_eq!(bytes(state, config), before);
    for command_kind in ["add", "default"] {
        let mut command = cli(home, state);
        if command_kind == "add" {
            command.args([
                "checkout",
                "add-source",
                &checkout.id,
                &pending.id,
                "github",
                &pending.locator,
            ]);
        } else {
            command.args(["checkout", "set-default", &checkout.id, &pending.id]);
        }
        let (ok, _, stderr) = finish(command, home, &format!("b3-pending-{command_kind}"));
        assert!(!ok);
        assert!(String::from_utf8_lossy(&stderr).contains(&EndpointError::Pending.to_string()));
        assert_eq!(bytes(state, config), before);
    }

    // Make the CURRENT default pending after the checkout already exists.
    let a = EndpointRegistry::new(config)
        .apply(
            &Request {
                operation_id: ulid::Ulid::new(),
                expected: Some(Expected::capture(&a).unwrap()),
                connection: a,
                intent: Intent::Explicit {
                    credential_ref: ACCOUNT.into(),
                    endpoint: Endpoint::Public,
                },
                link: Some(Link {
                    checkout_id: checkout.id.clone(),
                    make_default: true,
                }),
            },
            &Observation::Unavailable,
            &NoLinks,
        )
        .unwrap()
        .connection;
    let mut command = cli(home, state);
    command.args([
        "checkout",
        "add-source",
        &checkout.id,
        &b.id,
        "github",
        &b.locator,
    ]);
    let (ok, stdout, stderr) = finish(command, home, "b3-independent-add");
    assert!(ok, "{}", String::from_utf8_lossy(&stderr));
    let linked: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(linked["default_source"], a.id);
    assert!(
        state
            .checkout_registry
            .describe(&checkout.id)
            .unwrap()
            .source(&b.id)
            .is_some()
    );
    for target in [Some(b.id.as_str()), None] {
        let before = bytes(state, config);
        let (status, body) =
            request(state, "PUT", &defaults, json!({"connection_id":target})).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], EndpointError::Pending.to_string());
        assert_eq!(
            bytes(state, config),
            before,
            "displacing or clearing the pending default must not publish"
        );
    }

    // Ordinary clear/refill/explicit selection through both shipped adapters.
    let root = home.join("b3-positive-project");
    std::fs::create_dir(&root).unwrap();
    let positive = state
        .checkout_registry
        .register_registered_sources(
            config,
            &root,
            Some("b3-positive"),
            None,
            vec![source(&b)],
            Some(b.id.clone()),
        )
        .unwrap();
    let defaults = format!("/checkouts/{}/default-source", positive.id);
    for _ in 0..2 {
        let (status, body) = request(state, "PUT", &defaults, json!({"connection_id":null})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["default_source_cleared"], true);
        assert!(body.get("default_source").is_none());
    }
    let c = known(config, "b3-parent-c", false);
    let (status, body) = request(
        state,
        "PUT",
        &format!("/checkouts/{}/sources/{}", positive.id, c.id),
        json!({"provider":"github","locator":c.locator,"make_default":false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["default_source_cleared"], true);
    assert!(body.get("default_source").is_none());
    let mut command = cli(home, state);
    command.args(["checkout", "set-default", &positive.id, &c.id]);
    let (ok, stdout, stderr) = finish(command, home, "b3-select");
    assert!(ok, "{}", String::from_utf8_lossy(&stderr));
    let selected: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(selected["default_source"], c.id);
    assert_ne!(selected["default_source_cleared"], true);
    let mut command = cli(home, state);
    command.args(["checkout", "set-default", &positive.id, "--clear"]);
    let (ok, stdout, stderr) = finish(command, home, "b3-clear");
    assert!(ok, "{}", String::from_utf8_lossy(&stderr));
    let cleared: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(cleared["default_source_cleared"], true);

    // Authenticated locator PUT must fan out the actual normalized saved record.
    let mut changed = c.clone();
    changed.locator = "synthetic/relocated-c".into();
    changed
        .settings
        .as_object_mut()
        .unwrap()
        .remove("github_endpoint");
    let (status, body) = request(
        state,
        "PATCH",
        &format!("/provider-connections/{}", c.id),
        serde_json::to_value(&changed).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let current = config
        .load()
        .unwrap()
        .into_iter()
        .find(|row| row.id == c.id)
        .unwrap();
    assert_eq!(body, serde_json::to_value(&current).unwrap());
    assert!(
        hotsheet_ticketing::github_endpoint::State::load(&current)
            .unwrap()
            .unwrap()
            .operation
            .is_none()
    );
    assert_eq!(
        state
            .checkout_registry
            .describe(&positive.id)
            .unwrap()
            .source(&c.id)
            .unwrap()
            .locator,
        current.locator
    );
    let before = bytes(state, config);
    let late = state
        .checkout_registry
        .update_registered_source_locator(config, &c)
        .unwrap_err();
    assert!(
        matches!(
            late,
            hotsheet_ticketing::checkouts::CheckoutError::Invalid(_)
        ),
        "old locator is structurally stale before exact authority comparison"
    );
    let prewrite = state
        .checkout_registry
        .update_registered_source_locator(config, &changed)
        .unwrap_err();
    assert!(
        matches!(
            prewrite,
            hotsheet_ticketing::checkouts::CheckoutError::Endpoint(EndpointError::Conflict)
        ),
        "same locator but omitted normalized authority is not the actual returned candidate"
    );
    assert_eq!(
        bytes(state, config),
        before,
        "late caller cannot upgrade a saved candidate"
    );
    // A missing source is the established structural 400, not an endpoint 409.
    let (status, _) = request(
        state,
        "PUT",
        &format!("/checkouts/{}/sources/missing-b3", positive.id),
        json!({"provider":"github","locator":"synthetic/missing"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(bytes(state, config), before);
}
