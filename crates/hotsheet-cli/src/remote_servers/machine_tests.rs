use super::*;
use hotsheet_ticketing::orchestration::TargetFailure;
use hotsheet_ticketing::remote_servers::{RemoteCheckout, ReviewedRemoteIdentity};
use serde_json::json;

fn view(revision: u64, server_id: &str) -> RemoteView {
    RemoteView {
        id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        label: "Synthetic build host".into(),
        origin: "https://127.0.0.1:9".into(),
        identity: ReviewedRemoteIdentity {
            server_id: server_id.into(),
            ca_fingerprint: None,
        },
        revision,
        revoked: false,
        last_failure: None,
        checked_at_ms: None,
        confirmed_at_ms: None,
        last_confirmed_checkouts: Vec::new(),
        stale: false,
        credential_cleanup_pending: false,
        review_history: Vec::new(),
    }
}

fn checkout(id: &str, state: RemoteCheckoutState) -> RemoteCheckout {
    RemoteCheckout {
        id: id.into(),
        name: "synthetic".into(),
        sources: Vec::new(),
        state,
        resolution_metadata_version: None,
    }
}

fn discovery(checkouts: Option<Vec<RemoteCheckout>>) -> RemoteDiscovery {
    RemoteDiscovery {
        remote: view(3, "01ARZ3NDEKTSV4RRFFQ69G5FB0"),
        checkouts,
        host_name: Some("Build Host".into()),
        failure: None,
    }
}

#[test]
fn hs2_vq92xz_only_the_three_machine_routes_with_their_own_query() {
    assert_eq!(
        MachineRead::Health.server_path(None).as_deref(),
        Some("/host-health")
    );
    let cpu = |after, wait_ms| MachineRead::Cpu { after, wait_ms };
    assert_eq!(
        cpu(None, None).server_path(None).as_deref(),
        Some("/host-health/cpu")
    );
    assert_eq!(
        cpu(Some(41), Some(3_000)).server_path(None).as_deref(),
        Some("/host-health/cpu?after=41&timeout_ms=3000")
    );
    // A wait is capped at the shared long-poll bound.
    assert_eq!(
        cpu(Some(41), Some(60_000)).server_path(None).as_deref(),
        Some("/host-health/cpu?after=41&timeout_ms=8000")
    );
    let capacity = MachineRead::Capacity {
        after: None,
        wait_ms: None,
        claim_namespaces: Vec::new(),
    };
    // Capacity needs a checkout to go through.
    assert_eq!(capacity.server_path(None), None);
    assert_eq!(
        capacity.server_path(Some("proj-1a2b")).as_deref(),
        Some("/checkouts/proj-1a2b/target-capacity")
    );
    let waiting = MachineRead::Capacity {
        after: Some("v1&x=y".into()),
        wait_ms: Some(20_000),
        claim_namespaces: vec!["build.cache".into(), "gpu".into()],
    };
    // Every value is encoded: none can add a parameter or move the route.
    assert_eq!(
        waiting.server_path(Some("../admin")).as_deref(),
        Some(
            "/checkouts/..%2Fadmin/target-capacity?claim_namespaces=build.cache%2Cgpu&after=v1%26x%3Dy&wait_ms=8000"
        )
    );
}

#[test]
fn hs2_vq92xz_the_binding_is_rechecked_and_capacity_goes_through_an_available_checkout() {
    let connection = view(3, "01ARZ3NDEKTSV4RRFFQ69G5FB0");
    // The first available checkout, skipping one that is not.
    let found = discovery(Some(vec![
        checkout("gone-1", RemoteCheckoutState::Inaccessible),
        checkout("first-2", RemoteCheckoutState::Available),
        checkout("second-3", RemoteCheckoutState::Available),
    ]));
    assert_eq!(
        checked_binding(&found, &connection).unwrap().as_deref(),
        Some("first-2")
    );
    // None available, or none listed: no checkout, still a readable machine.
    assert_eq!(
        checked_binding(
            &discovery(Some(vec![checkout(
                "gone-1",
                RemoteCheckoutState::IdentityChanged
            )])),
            &connection
        )
        .unwrap(),
        None
    );
    assert_eq!(
        checked_binding(&discovery(Some(Vec::new())), &connection).unwrap(),
        None
    );
    // A binding reviewed (new revision) or re-identified since discovery is refused.
    for changed in [
        view(4, "01ARZ3NDEKTSV4RRFFQ69G5FB0"),
        view(3, "01ARZ3NDEKTSV4RRFFQ69G5FB1"),
    ] {
        assert!(matches!(
            checked_binding(&found, &changed),
            Err(MachineError::Changed)
        ));
    }
    // A revoked binding and a failed discovery are refused before any read.
    let mut revoked = discovery(None);
    revoked.remote.revoked = true;
    assert!(matches!(
        checked_binding(&revoked, &connection),
        Err(MachineError::Remote(RemoteError::Revoked))
    ));
    let mut expired = discovery(None);
    expired.failure = Some(RemoteFailure::SessionExpired);
    let error = checked_binding(&expired, &connection).unwrap_err();
    assert!(error.to_string().contains("session_expired"), "{error}");
}

#[test]
fn hs2_vq92xz_a_refusal_keeps_only_the_status_and_a_stable_code() {
    let health = MachineRead::Health;
    assert_eq!(
        reading(&health, 200, br#"{"cpu":{"percent":12}}"#).unwrap(),
        json!({"cpu":{"percent":12}})
    );
    assert!(matches!(
        reading(&health, 200, b"not json"),
        Err(MachineError::Remote(RemoteError::Verification(
            RemoteFailure::InvalidResponse
        )))
    ));
    let refused = reading(
        &health,
        401,
        br#"{"code":"session_expired","error":"synthetic peer message"}"#,
    )
    .unwrap_err();
    let text = refused.to_string();
    assert_eq!(
        text,
        "the remote server refused the health read (401, session_expired)"
    );
    // A code that is not a plain identifier, and the peer's message, never reach the caller.
    let odd = reading(&health, 403, br#"{"code":"Secret Value!","error":"x"}"#).unwrap_err();
    assert_eq!(
        odd.to_string(),
        "the remote server refused the health read (403)"
    );
    assert_eq!(
        reading(
            &MachineRead::Cpu {
                after: None,
                wait_ms: None
            },
            500,
            b""
        )
        .unwrap_err()
        .to_string(),
        "the remote server refused the cpu read (500)"
    );
    assert_eq!(
        MachineError::Transport(TargetFailure::Unreachable).to_string(),
        "remote machine read failed: unreachable"
    );
    assert!(
        MachineError::NoCheckout
            .to_string()
            .starts_with("no_available_checkout")
    );
    assert!(
        MachineError::Changed
            .to_string()
            .starts_with("remote_changed")
    );
}

#[test]
fn hs2_vq92xz_the_report_names_the_machine_and_only_its_readings() {
    let found = discovery(None);
    let report = report(
        &found,
        vec![("health", json!({"ok": true})), ("cpu", json!({"seq": 2}))],
        true,
    );
    assert_eq!(
        report,
        json!({
            "binding_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "label": "Synthetic build host",
            "server_id": "01ARZ3NDEKTSV4RRFFQ69G5FB0",
            "host_name": "Build Host",
            "health": {"ok": true},
            "cpu": {"seq": 2},
            "capacity_unavailable": "no_available_checkout",
        })
    );
    // Without a name or an unavailable capacity, those keys are plain.
    let mut nameless = discovery(None);
    nameless.host_name = None;
    let plain = super::report(&nameless, Vec::new(), false);
    assert_eq!(plain["host_name"], Value::Null);
    assert!(plain.get("capacity_unavailable").is_none());
}

/// HS2-SW03MK: the HTTP route and the MCP action name readings as a JSON body: unknown
/// fields are refused, a cursor is a number or text, and the same combinations the CLI
/// refuses are refused before any request.
#[test]
fn hs2_sw03mk_a_machine_query_reads_from_json_and_refuses_what_does_not_fit() {
    let query = |body: Value| serde_json::from_value::<MachineQuery>(body);
    let (reads, optional) = query(json!({})).unwrap().reads().unwrap();
    assert!(optional);
    assert_eq!(
        reads.iter().map(MachineRead::key).collect::<Vec<_>>(),
        ["health", "cpu", "capacity"]
    );
    let (reads, optional) = query(json!({"cpu": true, "after": 7, "wait_ms": 20_000}))
        .unwrap()
        .reads()
        .unwrap();
    assert!(!optional);
    assert_eq!(
        reads,
        [MachineRead::Cpu {
            after: Some(7),
            wait_ms: Some(20_000)
        }]
    );
    // The wait is capped where the path is built.
    assert_eq!(
        reads[0].server_path(None).unwrap(),
        "/host-health/cpu?after=7&timeout_ms=8000"
    );
    let (reads, _) = query(json!({"capacity": true, "after": "ab12", "wait_ms": 1000,
        "claim_namespaces": ["vm"]}))
    .unwrap()
    .reads()
    .unwrap();
    assert_eq!(
        reads,
        [MachineRead::Capacity {
            after: Some("ab12".into()),
            wait_ms: Some(1000),
            claim_namespaces: vec!["vm".into()]
        }]
    );
    // Refused before any request.
    assert!(query(json!({"credentials": {}})).is_err());
    assert!(query(json!({"health": "yes"})).is_err());
    for body in [
        json!({"after": 7}),
        json!({"cpu": true, "capacity": true, "after": 7}),
        json!({"cpu": true, "after": "v1"}),
        json!({"cpu": true, "wait_ms": 1000}),
        json!({"health": true, "claim_namespaces": ["vm"]}),
    ] {
        let refused = query(body.clone()).unwrap().reads().unwrap_err();
        assert!(matches!(refused, MachineError::Invalid(_)), "{body}");
    }
    assert_eq!(
        query(json!({"after": 7}))
            .unwrap()
            .reads()
            .unwrap_err()
            .to_string(),
        "after (a cursor) needs exactly one of cpu or capacity"
    );
}
