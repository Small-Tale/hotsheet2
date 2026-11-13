//! HS2-FMHH9Y: the shared request workflow every surface answers through, and the
//! public API a scheduler uses to treat a held resource as unavailable capacity.

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use hotsheet_resource_claims::{
    ClaimError, ClaimService, Guarantee, MetadataValue, Namespace, ResourceRef, WaitOutcome, held,
    http_status, is_path, parse_duration, resolve_backend, wait_until_free,
};
use serde_json::{Value, json};

const PROMPT: Duration = Duration::from_secs(20);

fn target(text: &str) -> ResourceRef {
    ResourceRef::parse(text).unwrap()
}

fn acquire(service: &ClaimService, path: &str, body: Value) -> Value {
    service
        .request("POST", &format!("/resource-claims/{path}/acquire"), &body)
        .unwrap()
}

#[test]
fn hs2_fmhh9y_request_workflow_walks_acquire_show_list_renew_release() {
    let home = tempfile::tempdir().unwrap();
    let service = ClaimService::new(home.path());

    let free = service
        .request("GET", "/resource-claims/vms/mac-1", &Value::Null)
        .unwrap();
    assert_eq!(free["held"], false);
    assert_eq!(free["claim"], Value::Null);
    assert_eq!(free["target"], "vms/mac-1");
    assert_eq!(free["capabilities"]["guarantee"], "exclusive_with_fencing");
    assert_eq!(free["capabilities"]["wait"], "push_signal");

    let grant = acquire(
        &service,
        "vms/mac-1",
        json!({
            "duration_ms": 60_000,
            "metadata": { "note": "ui", "worker": "hmac-sha256:w-1", "label": "enc:v1:Build" },
            // A mutating MCP call adds its actor; claims carry identity as metadata.
            "actor": { "role": "ai", "id": "agent" },
        }),
    );
    assert_eq!(grant["acquired"], true);
    assert_eq!(grant["namespace"], "vms");
    assert_eq!(grant["resource"], "mac-1");
    assert_eq!(grant["fencing_token"], 1);
    assert_eq!(grant["metadata"]["note"], "plain:ui");
    assert!(grant["claim_id"].is_string() && grant["expires_at"].is_string());
    assert_eq!(
        grant["expires_at_ms"].as_u64().unwrap() - grant["acquired_at_ms"].as_u64().unwrap(),
        60_000
    );
    let token = grant["renewal_token"].as_str().unwrap().to_owned();
    let codecs = resolve_backend(home.path(), &Namespace::parse("vms").unwrap())
        .unwrap()
        .codecs()
        .unwrap();
    assert!(codecs.hmac_matches(grant["metadata"]["worker"].as_str().unwrap(), "w-1"));
    assert_eq!(
        codecs
            .decode(grant["metadata"]["label"].as_str().unwrap())
            .unwrap(),
        MetadataValue::Decrypted("Build".into())
    );

    // A second acquire is an ordinary answer naming the holder, never the token.
    let refused = acquire(&service, "vms/mac-1", json!({ "duration_ms": 60_000 }));
    assert_eq!(refused["acquired"], false);
    assert_eq!(refused["code"], "held_by");
    assert_eq!(refused["holder"]["claim_id"], grant["claim_id"]);
    assert_eq!(refused["holder"]["metadata"], grant["metadata"]);
    assert!(refused["error"].as_str().unwrap().contains("held by claim"));
    assert!(!refused.to_string().contains(&token));

    let shown = service
        .request("GET", "/resource-claims/vms/mac-1", &Value::Null)
        .unwrap();
    assert_eq!(shown["held"], true);
    assert_eq!(shown["claim"]["claim_id"], grant["claim_id"]);
    let listed = service
        .request("GET", "/resource-claims/vms", &Value::Null)
        .unwrap();
    assert_eq!(listed["namespace"], "vms");
    assert_eq!(listed["claims"].as_array().unwrap().len(), 1);
    assert!(!shown.to_string().contains(&token) && !listed.to_string().contains(&token));

    let renewed = service
        .request(
            "POST",
            "/resource-claim-token/renew",
            &json!({ "renewal_token": token, "duration_ms": 120_000 }),
        )
        .unwrap();
    assert_eq!(renewed["claim"]["claim_id"], grant["claim_id"]);
    assert!(renewed["claim"]["expires_at_ms"].as_u64() > grant["expires_at_ms"].as_u64());
    let renewed = service
        .request(
            "POST",
            "/resource-claims/vms/mac-1/renew",
            &json!({ "renewal_token": token, "duration_ms": 60_000 }),
        )
        .unwrap();
    assert_eq!(renewed["claim"]["fencing_token"], 1);

    let released = service
        .request(
            "POST",
            "/resource-claim-token/release",
            &json!({ "renewal_token": token }),
        )
        .unwrap();
    assert_eq!(released["released"], true);
    assert_eq!(released["claim"]["claim_id"], grant["claim_id"]);
    let listed = service
        .request("GET", "/resource-claims/vms", &Value::Null)
        .unwrap();
    assert_eq!(listed["claims"], json!([]));
    let error = service
        .request(
            "POST",
            "/resource-claim-token/release",
            &json!({ "renewal_token": token }),
        )
        .unwrap_err();
    assert_eq!(error.code(), "token_invalid");
}

#[test]
fn hs2_fmhh9y_a_token_is_refused_for_any_other_claim_or_route() {
    let home = tempfile::tempdir().unwrap();
    let service = ClaimService::new(home.path());
    let one = acquire(&service, "vms/mac-1", json!({ "duration_ms": 60_000 }));
    let two = acquire(&service, "vms/mac-2", json!({ "duration_ms": 60_000 }));
    let token_one = one["renewal_token"].as_str().unwrap();

    // Claim one's token aimed at claim two's routes.
    for action in ["renew", "release"] {
        let error = service
            .request(
                "POST",
                &format!("/resource-claims/vms/mac-2/{action}"),
                &json!({ "renewal_token": token_one, "duration_ms": 60_000 }),
            )
            .unwrap_err();
        assert_eq!(error.code(), "token_invalid", "{action}");
        assert_eq!(http_status(&error), 403);
    }
    let error = service.renew(
        token_one,
        Duration::from_secs(60),
        Some(&target("gpus/mac-1")),
    );
    assert_eq!(error.unwrap_err().code(), "token_invalid");
    // Both claims are untouched, and each token still works for its own.
    let listed = service.list(&Namespace::parse("vms").unwrap()).unwrap();
    assert_eq!(listed.len(), 2);
    service
        .release(two["renewal_token"].as_str().unwrap(), None)
        .unwrap();
    service
        .release(token_one, Some(&target("vms/mac-1")))
        .unwrap();

    // The token routes do nothing but renew and release.
    for (method, path) in [
        ("POST", "/resource-claim-token/acquire"),
        ("POST", "/resource-claim-token/show"),
        ("GET", "/resource-claim-token/renew"),
        ("GET", "/resource-claim-token"),
        ("POST", "/resource-claims/vms/mac-1/steal"),
        ("DELETE", "/resource-claims/vms/mac-1"),
        ("GET", "/resource-claims"),
    ] {
        let error = service
            .request(
                method,
                path,
                &json!({ "renewal_token": token_one, "duration_ms": 1 }),
            )
            .unwrap_err();
        assert_eq!(error.code(), "unsupported", "{method} {path}");
    }
}

#[test]
fn hs2_fmhh9y_request_bodies_are_validated() {
    let home = tempfile::tempdir().unwrap();
    let service = ClaimService::new(home.path());
    for body in [
        json!({}),
        json!({ "duration_ms": "60s" }),
        json!({ "duration_ms": -1 }),
        json!({ "duration_ms": 0 }),
        json!({ "duration_ms": 60_000, "wait_ms": "soon" }),
        json!({ "duration_ms": 60_000, "metadata": ["a"] }),
        json!({ "duration_ms": 60_000, "metadata": { "a": 1 } }),
        json!({ "duration_ms": 60_000, "metadata": { "Bad Key": "x" } }),
    ] {
        let error = service
            .request("POST", "/resource-claims/vms/mac-1/acquire", &body)
            .unwrap_err();
        assert_eq!(error.code(), "invalid", "{body}");
        assert_eq!(http_status(&error), 400);
    }
    for path in [
        "/resource-claims/VMS/x/acquire",
        "/resource-claims/vms/a%20b/acquire",
    ] {
        let error = service
            .request("POST", path, &json!({ "duration_ms": 1 }))
            .unwrap_err();
        assert_eq!(error.code(), "invalid", "{path}");
    }
    let error = service
        .request(
            "POST",
            "/resource-claim-token/renew",
            &json!({ "duration_ms": 1 }),
        )
        .unwrap_err();
    assert_eq!(error.code(), "invalid");
    let error = service
        .request(
            "POST",
            "/resource-claim-token/renew",
            &json!({ "renewal_token": "x", "duration_ms": 1 }),
        )
        .unwrap_err();
    assert_eq!(error.code(), "token_invalid");
    assert!(!home.path().join("resource-claims/namespaces").exists());
}

#[test]
fn hs2_fmhh9y_a_bounded_service_answers_wait_timeout_and_frees_by_signal() {
    let home = tempfile::tempdir().unwrap();
    let service = ClaimService::new(home.path());
    let bounded = ClaimService::new(home.path()).with_max_wait(Duration::from_millis(300));
    let held_grant = acquire(&service, "vms/mac-1", json!({ "duration_ms": 120_000 }));

    // The caller asked for a minute; the bound answers after its own limit.
    let started = Instant::now();
    let refused = acquire(
        &bounded,
        "vms/mac-1",
        json!({ "duration_ms": 1_000, "wait_ms": 60_000 }),
    );
    assert!(started.elapsed() >= Duration::from_millis(300) && started.elapsed() < PROMPT);
    assert_eq!(refused["acquired"], false);
    assert_eq!(refused["code"], "wait_timeout");
    assert_eq!(refused["holder"]["claim_id"], held_grant["claim_id"]);

    let waited = bounded
        .request(
            "POST",
            "/resource-claims/vms/mac-1/wait",
            &json!({ "wait_ms": 60_000 }),
        )
        .unwrap();
    assert_eq!(waited["held"], true);
    assert_eq!(waited["claim"]["claim_id"], held_grant["claim_id"]);

    // An unbounded wait is ended by the release, long before the lease would lapse.
    let (done, result) = mpsc::channel();
    let waiter = service.clone();
    std::thread::spawn(move || {
        let answer = acquire(
            &waiter,
            "vms/mac-1",
            json!({ "duration_ms": 1_000, "wait_ms": 110_000 }),
        );
        done.send(answer).unwrap();
    });
    let waiters = home.path().join("resource-claims/namespaces/vms/waiters");
    let started = Instant::now();
    while std::fs::read_dir(&waiters).map_or(0, Iterator::count) == 0 {
        assert!(started.elapsed() < PROMPT, "waiter never registered");
        std::thread::yield_now();
    }
    service
        .release(held_grant["renewal_token"].as_str().unwrap(), None)
        .unwrap();
    let granted = result.recv_timeout(PROMPT).unwrap();
    assert_eq!(granted["acquired"], true);
    assert_eq!(granted["fencing_token"], 2);
}

#[test]
fn hs2_fmhh9y_schedulers_can_check_and_await_a_resource_without_acquiring() {
    let home = tempfile::tempdir().unwrap();
    let vm = target("vms/mac-1");
    assert_eq!(held(home.path(), &vm).unwrap(), None);
    assert_eq!(
        wait_until_free(home.path(), &vm, PROMPT).unwrap(),
        WaitOutcome::Free
    );

    let service = ClaimService::new(home.path());
    let grant = service
        .acquire(&vm, Duration::from_millis(700), None, &BTreeMap::new())
        .unwrap();
    let holder = held(home.path(), &vm).unwrap().expect("held");
    assert_eq!(holder, grant.claim);
    assert_eq!(holder.guarantee, Guarantee::ExclusiveWithFencing);
    assert!(
        holder.expires_at_ms > holder.acquired_at_ms,
        "held until a known instant"
    );

    let outcome = wait_until_free(home.path(), &vm, Duration::from_millis(50)).unwrap();
    assert_eq!(
        outcome,
        WaitOutcome::TimedOut {
            holder: Box::new(holder.clone())
        }
    );
    // Free again when the lease lapses; checking never took the resource.
    assert_eq!(
        wait_until_free(home.path(), &vm, PROMPT).unwrap(),
        WaitOutcome::Free
    );
    assert_eq!(held(home.path(), &vm).unwrap(), None);
    let next = service
        .acquire(&vm, Duration::from_secs(60), None, &BTreeMap::new())
        .unwrap();
    assert_eq!(next.claim.fencing_token, 2);

    // The seam for backend configurations: every namespace resolves to the machine backend.
    let backend = resolve_backend(home.path(), &Namespace::parse("anything").unwrap()).unwrap();
    assert_eq!(backend.capabilities().backend, "local");
    assert_eq!(backend.get(&vm).unwrap(), Some(next.claim));
}

#[test]
fn hs2_fmhh9y_durations_statuses_and_paths() {
    for (text, ms) in [
        ("500ms", 500),
        ("90s", 90_000),
        ("30m", 1_800_000),
        ("1h30m", 5_400_000),
        ("2d", 172_800_000),
        (" 1m30s ", 90_000),
        ("0s", 0),
    ] {
        assert_eq!(
            parse_duration(text).unwrap(),
            Duration::from_millis(ms),
            "{text}"
        );
    }
    for text in [
        "",
        "30",
        "m",
        "1x",
        "-5m",
        "1.5h",
        "5 m",
        "99999999999999999999s",
        "18446744073709551615d",
    ] {
        assert_eq!(
            parse_duration(text).unwrap_err().code(),
            "invalid",
            "{text}"
        );
    }
    let statuses = [
        (ClaimError::Expired, 410),
        (ClaimError::NotHolder, 409),
        (ClaimError::TokenInvalid, 403),
        (ClaimError::BackendUnreachable("down".into()), 503),
        (ClaimError::Unsupported("no".into()), 501),
        (ClaimError::Invalid("bad".into()), 400),
        (ClaimError::Capacity("full".into()), 507),
        (ClaimError::Corrupt, 500),
    ];
    for (error, status) in statuses {
        assert_eq!(http_status(&error), status, "{}", error.code());
    }
    for path in [
        "/resource-claims/vms",
        "/resource-claims/vms/x/acquire",
        "/resource-claim-token/renew",
    ] {
        assert!(is_path(path), "{path}");
    }
    for path in [
        "/resource-claims",
        "/resource-claimsx/vms",
        "/tickets/resource-claims/x",
        "/resource-claim-token",
    ] {
        assert!(!is_path(path), "{path}");
    }
}
