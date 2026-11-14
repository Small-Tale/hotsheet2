//! HS2-H3WPPY: the claim transition matrix on the local backend with an injected clock.
//! States per resource: never-claimed, held, lapsed (expired, slot not yet reused),
//! released. Transitions: acquire, renew, release, clock passing expiry.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use hotsheet_resource_claims::{
    AcquireRequest, ClaimBackend, ClaimKey, FileKeyProvider, Guarantee, KeyProvider, LocalBackend,
    ManualClock, MetadataCodec, MetadataCodecs, MetadataValue, Namespace, ResourceRef,
    StaticKeyProvider, WaitMechanism, parse_renewal_token, validate_metadata,
};

const START_MS: u64 = 1_800_000_000_000;

struct Fixture {
    dir: tempfile::TempDir,
    clock: Arc<ManualClock>,
    backend: LocalBackend,
}

fn key(byte: u8) -> Arc<StaticKeyProvider> {
    Arc::new(StaticKeyProvider::new(ClaimKey::from_bytes([byte; 32])))
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(START_MS));
    let backend = LocalBackend::new(dir.path().join("resource-claims"), clock.clone(), key(7));
    Fixture {
        dir,
        clock,
        backend,
    }
}

fn target(text: &str) -> ResourceRef {
    ResourceRef::parse(text).unwrap()
}

fn lease(seconds: u64) -> AcquireRequest {
    AcquireRequest::new(Duration::from_secs(seconds))
}

fn state_text(fixture: &Fixture, namespace: &str) -> String {
    std::fs::read_to_string(
        fixture
            .dir
            .path()
            .join("resource-claims/namespaces")
            .join(namespace)
            .join("state.json"),
    )
    .unwrap()
}

#[test]
fn hs2_h3wppy_acquire_renew_release_walks_the_held_state() {
    let f = fixture();
    let vm = target("vms/mac-1");
    assert_eq!(f.backend.get(&vm).unwrap(), None);

    let grant = f.backend.acquire(&vm, &lease(60)).unwrap();
    assert_eq!(grant.claim.fencing_token, 1);
    assert_eq!(grant.claim.guarantee, Guarantee::ExclusiveWithFencing);
    assert_eq!(grant.claim.acquired_at_ms, START_MS);
    assert_eq!(grant.claim.expires_at_ms, START_MS + 60_000);
    assert_eq!(grant.claim.expires_at, "2027-01-15T08:01:00Z");
    assert_eq!(grant.claim.target(), "vms/mac-1");
    assert_eq!(f.backend.get(&vm).unwrap(), Some(grant.claim.clone()));
    assert_eq!(
        f.backend.list(vm.namespace()).unwrap(),
        vec![grant.claim.clone()]
    );
    f.backend.verify_fencing(&vm, 1).unwrap();

    f.clock.advance_ms(30_000);
    let renewed = f
        .backend
        .renew(&grant.renewal_token, Duration::from_secs(60))
        .unwrap();
    assert_eq!(renewed.claim_id, grant.claim.claim_id);
    assert_eq!(renewed.fencing_token, 1, "renewal keeps the fencing token");
    assert_eq!(renewed.expires_at_ms, START_MS + 90_000);
    assert_eq!(renewed.acquired_at_ms, START_MS);

    let released = f.backend.release(&grant.renewal_token).unwrap();
    assert_eq!(released.claim_id, grant.claim.claim_id);
    assert_eq!(f.backend.get(&vm).unwrap(), None);
    assert!(f.backend.list(vm.namespace()).unwrap().is_empty());
}

#[test]
fn hs2_h3wppy_try_now_conflict_names_the_holder_and_its_metadata() {
    let f = fixture();
    let vm = target("vms/mac-1");
    let codecs = f.backend.codecs().unwrap();
    let metadata = BTreeMap::from([
        (
            "label".to_owned(),
            codecs.encode_input("enc:v1:build").unwrap(),
        ),
        (
            "worker".to_owned(),
            codecs.encode_input("hmac-sha256:w-1").unwrap(),
        ),
        (
            "purpose".to_owned(),
            codecs.encode_input("ui tests").unwrap(),
        ),
    ]);
    let held = f
        .backend
        .acquire(&vm, &lease(60).with_metadata(metadata.clone()))
        .unwrap();

    let error = f.backend.acquire(&vm, &lease(60)).unwrap_err();
    assert_eq!(error.code(), "held_by");
    let holder = error.holder().unwrap();
    assert_eq!(holder, &held.claim);
    assert_eq!(holder.metadata, metadata);
    assert_eq!(holder.metadata["purpose"], "plain:ui tests");
    assert_eq!(
        codecs.decode(&holder.metadata["label"]).unwrap(),
        MetadataValue::Decrypted("build".into())
    );
    assert!(codecs.hmac_matches(&holder.metadata["worker"], "w-1"));
    // The refused acquire changed nothing: the counter did not move.
    f.backend.release(&held.renewal_token).unwrap();
    assert_eq!(
        f.backend
            .acquire(&vm, &lease(60))
            .unwrap()
            .claim
            .fencing_token,
        2
    );
}

#[test]
fn hs2_h3wppy_expiry_frees_the_resource_and_fences_the_old_holder() {
    let f = fixture();
    let vm = target("vms/mac-1");
    let first = f.backend.acquire(&vm, &lease(10)).unwrap();

    f.clock.advance_ms(9_999);
    assert!(
        f.backend.get(&vm).unwrap().is_some(),
        "one ms before expiry"
    );
    f.clock.advance_ms(1);
    assert_eq!(f.backend.get(&vm).unwrap(), None, "at the expiry instant");
    assert!(f.backend.list(vm.namespace()).unwrap().is_empty());
    assert_eq!(
        f.backend.verify_fencing(&vm, 1).unwrap_err().code(),
        "expired"
    );

    let second = f.backend.acquire(&vm, &lease(10)).unwrap();
    assert_eq!(second.claim.fencing_token, 2);
    assert_ne!(second.claim.claim_id, first.claim.claim_id);

    // The stale holder is fenced out everywhere.
    assert_eq!(
        f.backend.verify_fencing(&vm, 1).unwrap_err().code(),
        "not_holder"
    );
    f.backend.verify_fencing(&vm, 2).unwrap();
    assert_eq!(
        f.backend.verify_fencing(&vm, 3).unwrap_err().code(),
        "invalid"
    );
    assert_eq!(
        f.backend.verify_fencing(&vm, 0).unwrap_err().code(),
        "invalid"
    );
    let renew = f
        .backend
        .renew(&first.renewal_token, Duration::from_secs(10));
    assert_eq!(renew.unwrap_err().code(), "not_holder");
    assert_eq!(
        f.backend.release(&first.renewal_token).unwrap_err().code(),
        "not_holder"
    );
    // The stale holder's attempts did not disturb the live claim.
    assert_eq!(f.backend.get(&vm).unwrap(), Some(second.claim));
}

#[test]
fn hs2_h3wppy_renew_and_release_after_expiry_report_expired() {
    let f = fixture();
    let vm = target("vms/mac-1");
    let grant = f.backend.acquire(&vm, &lease(10)).unwrap();
    f.clock.advance_ms(10_000);

    // Nobody has taken the resource, but a lapsed lease cannot be revived.
    let renew = f
        .backend
        .renew(&grant.renewal_token, Duration::from_secs(10));
    assert_eq!(renew.unwrap_err().code(), "expired");
    assert_eq!(f.backend.get(&vm).unwrap(), None);
    let renew = f
        .backend
        .renew(&grant.renewal_token, Duration::from_secs(10));
    assert_eq!(renew.unwrap_err().code(), "expired", "repeatable");

    assert_eq!(
        f.backend.release(&grant.renewal_token).unwrap_err().code(),
        "expired"
    );
    // That release cleared the slot, so the token is now simply revoked.
    assert_eq!(
        f.backend.release(&grant.renewal_token).unwrap_err().code(),
        "token_invalid"
    );
    assert_eq!(
        f.backend
            .acquire(&vm, &lease(10))
            .unwrap()
            .claim
            .fencing_token,
        2
    );
}

#[test]
fn hs2_h3wppy_release_twice_revokes_the_token() {
    let f = fixture();
    let vm = target("vms/mac-1");
    let grant = f.backend.acquire(&vm, &lease(10)).unwrap();
    f.backend.release(&grant.renewal_token).unwrap();

    assert_eq!(
        f.backend.release(&grant.renewal_token).unwrap_err().code(),
        "token_invalid"
    );
    let renew = f
        .backend
        .renew(&grant.renewal_token, Duration::from_secs(10));
    assert_eq!(renew.unwrap_err().code(), "token_invalid");
    assert_eq!(
        f.backend.verify_fencing(&vm, 1).unwrap_err().code(),
        "expired"
    );
    assert_eq!(f.backend.get(&vm).unwrap(), None);
}

#[test]
fn hs2_h3wppy_a_token_only_works_for_its_own_claim() {
    let f = fixture();
    let one = target("vms/mac-1");
    let two = target("vms/mac-2");
    let grant_one = f.backend.acquire(&one, &lease(60)).unwrap();
    let grant_two = f.backend.acquire(&two, &lease(60)).unwrap();

    let parsed = parse_renewal_token(&grant_one.renewal_token).unwrap();
    assert_eq!(parsed.target, one);
    assert_eq!(parsed.claim_id, grant_one.claim.claim_id);

    // Re-address claim one's secret at claim two: the keyed digest no longer matches.
    let parts_one: Vec<&str> = grant_one.renewal_token.split('.').collect();
    let parts_two: Vec<&str> = grant_two.renewal_token.split('.').collect();
    let forged = [parts_two[0], parts_two[1], parts_two[2], parts_one[3]].join(".");
    assert_eq!(parse_renewal_token(&forged).unwrap().target, two);
    for attempt in [
        forged.as_str(),
        "",
        "not-a-token",
        "hsrc1.a.b.c",
        &format!("{}x", grant_one.renewal_token),
        &"h".repeat(2_000),
    ] {
        let renew = f.backend.renew(attempt, Duration::from_secs(60));
        assert_eq!(renew.unwrap_err().code(), "token_invalid", "{attempt}");
        assert_eq!(
            f.backend.release(attempt).unwrap_err().code(),
            "token_invalid"
        );
    }
    // A well-formed token for a namespace that was never written is invalid, and does
    // not create that namespace.
    let ghost = grant_one.renewal_token.replace(
        parts_one[1],
        "Z2hvc3RzL21hYy0x", // "ghosts/mac-1"
    );
    assert_eq!(
        f.backend.release(&ghost).unwrap_err().code(),
        "token_invalid"
    );
    assert!(
        !f.dir
            .path()
            .join("resource-claims/namespaces/ghosts")
            .exists()
    );

    // Both real claims survived every attempt, and each token still works for its own.
    assert_eq!(f.backend.list(one.namespace()).unwrap().len(), 2);
    f.backend.release(&grant_two.renewal_token).unwrap();
    assert_eq!(f.backend.get(&one).unwrap(), Some(grant_one.claim.clone()));
    f.backend.release(&grant_one.renewal_token).unwrap();
}

#[test]
fn hs2_h3wppy_only_a_keyed_digest_of_the_token_is_stored() {
    let f = fixture();
    let vm = target("vms/mac-1");
    let grant = f.backend.acquire(&vm, &lease(60)).unwrap();
    let stored = state_text(&f, "vms");
    let secret = grant.renewal_token.rsplit('.').next().unwrap();
    assert!(!stored.contains(&grant.renewal_token));
    assert!(!stored.contains(secret));
    assert!(stored.contains("token_digest"));

    // The digest is keyed: the same state under another key rejects the real token.
    let other = LocalBackend::new(
        f.dir.path().join("resource-claims"),
        f.clock.clone(),
        key(8),
    );
    let renew = other.renew(&grant.renewal_token, Duration::from_secs(60));
    assert_eq!(renew.unwrap_err().code(), "token_invalid");
    f.backend
        .renew(&grant.renewal_token, Duration::from_secs(60))
        .unwrap();
}

#[test]
fn hs2_h3wppy_empty_then_refill_keeps_the_fencing_counter_across_instances() {
    let f = fixture();
    let vm = target("vms/mac-1");
    let mut tokens = Vec::new();
    for round in 1..=4u64 {
        let grant = f.backend.acquire(&vm, &lease(60)).unwrap();
        assert_eq!(grant.claim.fencing_token, round);
        tokens.push(grant.renewal_token.clone());
        f.backend.release(&grant.renewal_token).unwrap();
        assert!(f.backend.list(vm.namespace()).unwrap().is_empty());
    }
    // A fresh process (a new backend over the same files) continues the counter, through
    // a lapse as well as a release.
    let reopened = LocalBackend::new(
        f.dir.path().join("resource-claims"),
        f.clock.clone(),
        key(7),
    );
    let fifth = reopened.acquire(&vm, &lease(1)).unwrap();
    assert_eq!(fifth.claim.fencing_token, 5);
    f.clock.advance_ms(1_000);
    assert_eq!(
        f.backend
            .acquire(&vm, &lease(1))
            .unwrap()
            .claim
            .fencing_token,
        6
    );
    for token in tokens {
        assert_eq!(f.backend.release(&token).unwrap_err().code(), "not_holder");
    }
}

#[test]
fn hs2_h3wppy_resources_and_namespaces_are_independent() {
    let f = fixture();
    let a = f.backend.acquire(&target("vms/mac-1"), &lease(60)).unwrap();
    let b = f.backend.acquire(&target("vms/mac-2"), &lease(5)).unwrap();
    let c = f
        .backend
        .acquire(&target("gpus/mac-1"), &lease(60))
        .unwrap();
    assert_eq!((a.claim.fencing_token, b.claim.fencing_token), (1, 1));
    assert_eq!(c.claim.fencing_token, 1);
    let names = |namespace: &str| -> Vec<String> {
        f.backend
            .list(&Namespace::parse(namespace).unwrap())
            .unwrap()
            .into_iter()
            .map(|claim| claim.resource)
            .collect()
    };
    assert_eq!(names("vms"), ["mac-1", "mac-2"]);
    assert_eq!(names("gpus"), ["mac-1"]);
    assert!(names("unused").is_empty());

    f.clock.advance_ms(5_000);
    assert_eq!(
        names("vms"),
        ["mac-1"],
        "only the lapsed lease left the list"
    );
    f.backend.release(&a.renewal_token).unwrap();
    assert!(names("vms").is_empty());
    assert_eq!(names("gpus"), ["mac-1"]);
}

#[test]
fn hs2_h3wppy_renew_sets_the_lease_from_the_backend_clock() {
    let f = fixture();
    let vm = target("vms/mac-1");
    let grant = f.backend.acquire(&vm, &lease(60)).unwrap();
    f.clock.advance_ms(1_000);
    let shorter = f
        .backend
        .renew(&grant.renewal_token, Duration::from_secs(5))
        .unwrap();
    assert_eq!(shorter.expires_at_ms, START_MS + 6_000);
    f.clock.advance_ms(4_999);
    let longer = f
        .backend
        .renew(&grant.renewal_token, Duration::from_millis(1))
        .unwrap();
    assert_eq!(longer.expires_at_ms, START_MS + 6_000);
    f.clock.advance_ms(1);
    let renew = f
        .backend
        .renew(&grant.renewal_token, Duration::from_secs(5));
    assert_eq!(renew.unwrap_err().code(), "expired");
}

#[test]
fn hs2_h3wppy_requests_are_validated_before_any_state_exists() {
    let f = fixture();
    let vm = target("vms/mac-1");
    for duration in [Duration::ZERO, Duration::from_secs(24 * 60 * 60 + 1)] {
        let error = f.backend.acquire(&vm, &AcquireRequest::new(duration));
        assert_eq!(error.unwrap_err().code(), "invalid");
    }
    let unprefixed = BTreeMap::from([("label".to_owned(), "raw".to_owned())]);
    let bad_key = BTreeMap::from([("Label".to_owned(), "plain:x".to_owned())]);
    let control = BTreeMap::from([("label".to_owned(), "plain:a\nb".to_owned())]);
    let too_big = BTreeMap::from([("label".to_owned(), format!("plain:{}", "x".repeat(4096)))]);
    let too_many: BTreeMap<String, String> = (0..17)
        .map(|n| (format!("k{n}"), "plain:v".to_owned()))
        .collect();
    for metadata in [unprefixed, bad_key, control, too_big, too_many] {
        assert_eq!(validate_metadata(&metadata).unwrap_err().code(), "invalid");
        let error = f.backend.acquire(&vm, &lease(60).with_metadata(metadata));
        assert_eq!(error.unwrap_err().code(), "invalid");
    }
    for name in [
        "", "vms", "/x", "vms/", "VMS/x", "vms/a b", "v..m/x", "vms/../x", "vms/.x",
    ] {
        assert_eq!(
            ResourceRef::parse(name).unwrap_err().code(),
            "invalid",
            "{name}"
        );
    }
    assert_eq!(target("vms/HS2-ABC123").to_string(), "vms/HS2-ABC123");
    // Reads and refused writes created nothing.
    assert_eq!(f.backend.get(&vm).unwrap(), None);
    assert!(!f.dir.path().join("resource-claims").exists());

    let capabilities = f.backend.capabilities();
    assert_eq!(capabilities.backend, "local");
    assert_eq!(capabilities.guarantee, Guarantee::ExclusiveWithFencing);
    assert_eq!(capabilities.wait, WaitMechanism::PushSignal);
    assert_eq!(capabilities.max_metadata_bytes, 4096);
    assert_eq!(capabilities.max_metadata_entries, 16);
    assert_eq!(capabilities.max_lease_ms, 86_400_000);
}

#[test]
fn hs2_h3wppy_corrupt_or_interrupted_state_is_never_overwritten() {
    let f = fixture();
    let vm = target("vms/mac-1");
    let grant = f.backend.acquire(&vm, &lease(60)).unwrap();
    let dir = f.dir.path().join("resource-claims/namespaces/vms");

    // A writer that died before its rename leaves a staged file; the published state
    // is still the last complete one.
    std::fs::write(dir.join(".state-crashed.tmp"), b"{\"schema_version\":1,").unwrap();
    assert_eq!(f.backend.get(&vm).unwrap(), Some(grant.claim.clone()));
    f.backend
        .renew(&grant.renewal_token, Duration::from_secs(60))
        .unwrap();

    let good = std::fs::read(dir.join("state.json")).unwrap();
    for bad in [&b"{not json"[..], br#"{"schema_version":2,"resources":{}}"#] {
        std::fs::write(dir.join("state.json"), bad).unwrap();
        assert_eq!(f.backend.get(&vm).unwrap_err().code(), "corrupt");
        assert_eq!(
            f.backend.list(vm.namespace()).unwrap_err().code(),
            "corrupt"
        );
        assert_eq!(
            f.backend.acquire(&vm, &lease(60)).unwrap_err().code(),
            "corrupt"
        );
        assert_eq!(
            f.backend.release(&grant.renewal_token).unwrap_err().code(),
            "corrupt"
        );
        assert_eq!(std::fs::read(dir.join("state.json")).unwrap(), bad);
    }
    std::fs::write(dir.join("state.json"), good).unwrap();
    f.backend.release(&grant.renewal_token).unwrap();
}

#[test]
fn hs2_h3wppy_a_full_namespace_refuses_new_resources_only() {
    let f = fixture();
    let first = f.backend.acquire(&target("pool/r0"), &lease(60)).unwrap();
    // Fill the namespace with released records without 4096 slow writes.
    let dir = f.dir.path().join("resource-claims/namespaces/pool");
    let mut state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("state.json")).unwrap()).unwrap();
    for n in 1..4096 {
        state["resources"][format!("r{n}")] = serde_json::json!({ "fencing": 3 });
    }
    std::fs::write(dir.join("state.json"), serde_json::to_vec(&state).unwrap()).unwrap();

    let error = f
        .backend
        .acquire(&target("pool/new"), &lease(60))
        .unwrap_err();
    assert_eq!(error.code(), "capacity");
    assert_eq!(
        f.backend
            .acquire(&target("pool/r9"), &lease(60))
            .unwrap()
            .claim
            .fencing_token,
        4
    );
    f.backend.release(&first.renewal_token).unwrap();
    assert_eq!(
        f.backend
            .list(&Namespace::parse("pool").unwrap())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn hs2_h3wppy_metadata_codecs_round_trip_and_stay_keyed() {
    let codecs = MetadataCodecs::new(&ClaimKey::from_bytes([1; 32])).unwrap();
    let other = MetadataCodecs::new(&ClaimKey::from_bytes([2; 32])).unwrap();

    assert_eq!(codecs.encode_input("hello").unwrap(), "plain:hello");
    assert_eq!(codecs.encode_input("plain:hello").unwrap(), "plain:hello");
    assert_eq!(
        codecs.decode("plain:hello").unwrap(),
        MetadataValue::Plain("hello".into())
    );

    let digest = codecs
        .encode(MetadataCodec::HmacSha256, "worker-1")
        .unwrap();
    assert!(digest.starts_with("hmac-sha256:") && digest.len() == 12 + 64);
    assert_eq!(digest, codecs.encode_input("hmac-sha256:worker-1").unwrap());
    assert!(!digest.contains("worker-1"));
    assert!(codecs.hmac_matches(&digest, "worker-1"));
    assert!(!codecs.hmac_matches(&digest, "worker-2"));
    assert!(
        !other.hmac_matches(&digest, "worker-1"),
        "keyed, not a plain hash"
    );
    assert!(!codecs.hmac_matches("plain:worker-1", "worker-1"));
    assert!(matches!(
        codecs.decode(&digest).unwrap(),
        MetadataValue::Hmac(_)
    ));

    let sealed = codecs.encode_input("enc:v1:Build agent").unwrap();
    let again = codecs.encode(MetadataCodec::EncV1, "Build agent").unwrap();
    assert!(sealed.starts_with("enc:v1:") && !sealed.contains("Build"));
    assert_ne!(sealed, again, "a fresh nonce per value");
    assert_eq!(
        codecs.decode(&sealed).unwrap(),
        MetadataValue::Decrypted("Build agent".into())
    );
    assert_eq!(other.decode(&sealed).unwrap_err().code(), "invalid");
    let mut altered = sealed.clone();
    altered.push('A');
    assert_eq!(codecs.decode(&altered).unwrap_err().code(), "invalid");
    assert_eq!(codecs.decode("enc:v1:AAAA").unwrap_err().code(), "invalid");
    assert_eq!(codecs.decode("raw").unwrap_err().code(), "invalid");
}

#[test]
fn hs2_h3wppy_the_machine_key_file_is_private_and_stable() {
    let dir = tempfile::tempdir().unwrap();
    let backend = LocalBackend::machine(dir.path());
    let vm = target("vms/mac-1");
    let grant = backend.acquire(&vm, &lease(60)).unwrap();
    let key_path = dir.path().join("resource-claims/key");
    assert_eq!(std::fs::metadata(&key_path).unwrap().len(), 32);

    // Another process on the machine derives the same key and accepts the token.
    let again = LocalBackend::machine(dir.path());
    again
        .renew(&grant.renewal_token, Duration::from_secs(60))
        .unwrap();
    let sealed = backend
        .codecs()
        .unwrap()
        .encode_input("enc:v1:label")
        .unwrap();
    assert_eq!(
        again.codecs().unwrap().decode(&sealed).unwrap(),
        MetadataValue::Decrypted("label".into())
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode();
        assert_eq!(mode(&key_path) & 0o777, 0o600);
        let namespace = dir.path().join("resource-claims/namespaces/vms");
        assert_eq!(mode(&namespace.join("state.json")) & 0o777, 0o600);
        assert_eq!(mode(&namespace) & 0o777, 0o700);

        // A key other users can read is refused rather than trusted.
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            again.acquire(&vm, &lease(60)).unwrap_err().code(),
            "invalid"
        );
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    std::fs::write(&key_path, b"short").unwrap();
    assert_eq!(
        FileKeyProvider::new(&key_path).key().unwrap_err().code(),
        "corrupt"
    );
}

/// HS2-YDZAFH: the backend enumerates the namespaces it keeps state for, sorted, without
/// creating anything; a directory that is not a namespace name is not one. A namespace
/// stays listed after its claims are released or expire (it simply holds none).
#[test]
fn hs2_ydzafh_the_backend_enumerates_its_namespaces() {
    let f = fixture();
    let root = f.dir.path().join("resource-claims");
    assert_eq!(f.backend.namespaces().unwrap(), Vec::<Namespace>::new());
    assert!(!root.exists(), "an enumeration creates nothing");
    let vm = f.backend.acquire(&target("vms/mac-1"), &lease(60)).unwrap();
    f.backend.acquire(&target("gpu/a100"), &lease(60)).unwrap();
    f.backend.acquire(&target("vms/mac-2"), &lease(60)).unwrap();
    let names = |f: &Fixture| {
        f.backend
            .namespaces()
            .unwrap()
            .into_iter()
            .map(|namespace| namespace.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&f), ["gpu", "vms"]);
    // Foreign entries next to the namespaces are ignored.
    std::fs::create_dir_all(root.join("namespaces/Not A Namespace")).unwrap();
    std::fs::write(root.join("namespaces/stray.txt"), b"x").unwrap();
    assert_eq!(names(&f), ["gpu", "vms"]);
    // Released and expired: still a namespace, now without live claims.
    f.backend.release(&vm.renewal_token).unwrap();
    f.clock.advance_ms(61_000);
    assert_eq!(names(&f), ["gpu", "vms"]);
    for namespace in f.backend.namespaces().unwrap() {
        assert!(f.backend.list(&namespace).unwrap().is_empty());
    }
    // Refilled.
    f.backend.acquire(&target("vms/mac-1"), &lease(60)).unwrap();
    assert_eq!(
        f.backend
            .list(&Namespace::parse("vms").unwrap())
            .unwrap()
            .len(),
        1
    );
}
