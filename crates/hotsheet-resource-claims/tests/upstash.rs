//! HS2-K9J1RK: the Upstash Redis REST backend against a faithful fake of the service
//! (`support/fake_upstash.rs`): real HTTP on loopback, the documented request and reply
//! shapes and status codes, a bearer-token check, rate limits, and pub/sub over
//! server-sent events. Synthetic names and tokens only. The Lua scripts themselves run only
//! in `upstash_live.rs`, against a real database.

mod support {
    pub mod fake_upstash;
}

use std::sync::Arc;
use std::time::{Duration, Instant};

use support::fake_upstash::{Fake, TOKEN};

use hotsheet_resource_claims::conformance::{self, Fixture};
use hotsheet_resource_claims::external::SharedSecrets;
use hotsheet_resource_claims::upstash::transport::{UreqTransport, validate_url};
use hotsheet_resource_claims::upstash::{self, UpstashBackend, scripts};
use hotsheet_resource_claims::{
    AcquireRequest, ClaimBackend, Namespace, ResourceRef, WaitMechanism, WaitOutcome,
};
use hotsheet_ticketing::claim_backends::{BackendConfiguration, BackendKind};
use hotsheet_ticketing::secrets::{SecretError, SecretStore};

fn backend_at(fake: &Fake, token: &str) -> UpstashBackend {
    UpstashBackend::new(
        "up",
        "hs2rc",
        Arc::new(UreqTransport::new(&fake.url, token)),
        None,
    )
}

fn backend(fake: &Fake) -> UpstashBackend {
    backend_at(fake, TOKEN)
}

fn target(text: &str) -> ResourceRef {
    ResourceRef::parse(text).unwrap()
}

fn lease(seconds: u64) -> AcquireRequest {
    AcquireRequest::new(Duration::from_secs(seconds))
}

fn unique_namespace() -> Namespace {
    Namespace::parse(&format!(
        "conf-{}",
        ulid::Ulid::new().to_string().to_lowercase()
    ))
    .unwrap()
}

#[test]
fn hs2_k9j1rk_the_backend_passes_the_conformance_suite_over_real_http() {
    let fake = Fake::start();
    let backend = backend(&fake);
    assert_eq!(backend.capabilities().wait, WaitMechanism::PushSignal);
    conformance::run(&Fixture {
        backend: &backend,
        namespace: unique_namespace(),
        advance: &std::thread::sleep,
        short_lease: Duration::from_millis(900),
    });
}

#[test]
fn hs2_k9j1rk_every_script_is_named_and_sent_verbatim() {
    for (name, script) in [
        ("acquire", scripts::ACQUIRE),
        ("renew", scripts::RENEW),
        ("release", scripts::RELEASE),
        ("get", scripts::GET),
        ("verify", scripts::VERIFY),
    ] {
        assert_eq!(
            script.lines().next().unwrap(),
            format!("-- hs2rc:{name}:v1")
        );
        assert!(
            script.contains("redis.call('TIME')"),
            "{name}: expiry is on the Redis clock"
        );
        assert!(
            !script.contains("os.time"),
            "{name}: never the client's clock"
        );
    }
}

#[test]
fn hs2_k9j1rk_the_keys_hold_names_only_and_the_token_never_crosses_the_wire() {
    let fake = Fake::start();
    let backend = backend(&fake);
    let mut metadata = std::collections::BTreeMap::new();
    metadata.insert("owner".to_owned(), "plain:synthetic-owner".to_owned());
    let grant = backend
        .acquire(&target("vms/mac-1"), &lease(60).with_metadata(metadata))
        .unwrap();
    backend
        .renew(&grant.renewal_token, Duration::from_secs(90))
        .unwrap();
    backend.release(&grant.renewal_token).unwrap();
    for key in fake.keys() {
        assert!(key.starts_with("hs2rc:v1:"), "{key}");
        assert!(!key.contains("hsrc1"), "{key}");
    }
    let evals = fake.evals.lock().unwrap().clone();
    assert!(evals.len() >= 3);
    for eval in &evals {
        let wire = eval.to_string();
        assert!(!wire.contains(&grant.renewal_token), "{wire}");
        assert!(!wire.contains("hsrc1."), "no token of any shape: {wire}");
    }
    // The fencing counter and the claim live under separate keys; the counter has no expiry.
    let keys = fake.keys();
    assert!(
        keys.contains(&"hs2rc:v1:vms:f:mac-1".to_owned()),
        "{keys:?}"
    );
    assert!(keys.contains(&"hs2rc:v1:namespaces".to_owned()), "{keys:?}");
}

#[test]
fn hs2_k9j1rk_a_wrong_token_is_a_401_that_names_the_next_step() {
    let fake = Fake::start();
    let error = backend_at(&fake, "wrong-token")
        .acquire(&target("vms/one"), &lease(60))
        .unwrap_err();
    assert_eq!(error.code(), "unsupported");
    assert!(
        error
            .to_string()
            .contains("resource backend secret up token"),
        "{error}"
    );
    assert!(
        !error.to_string().contains("wrong-token"),
        "never echoes the token"
    );
}

#[test]
fn hs2_k9j1rk_a_rate_limit_is_retried_a_bounded_number_of_times() {
    let fake = Fake::start();
    let backend = backend(&fake);
    fake.faults
        .rate_limit_next
        .store(2, std::sync::atomic::Ordering::SeqCst);
    backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    assert_eq!(
        fake.count("eval acquire"),
        1,
        "the refused attempts never reached Redis"
    );

    fake.faults
        .rate_limit_next
        .store(10, std::sync::atomic::Ordering::SeqCst);
    let error = backend.acquire(&target("vms/two"), &lease(60)).unwrap_err();
    assert_eq!(error.code(), "backend_unreachable");
    assert!(error.to_string().contains("rate limited"), "{error}");
    assert!(
        fake.faults
            .rate_limit_next
            .load(std::sync::atomic::Ordering::SeqCst)
            >= 7,
        "gave up after three attempts, not ten"
    );
}

#[test]
fn hs2_k9j1rk_an_unreachable_service_fails_closed_naming_the_configuration() {
    let fake = Fake::start();
    let backend = backend(&fake);
    fake.stop();
    std::thread::sleep(Duration::from_millis(100));
    let error = backend.get(&target("vms/one")).unwrap_err();
    assert_eq!(error.code(), "backend_unreachable");
    assert!(error.to_string().contains("configuration 'up'"), "{error}");
}

#[test]
fn hs2_k9j1rk_a_lapsed_claim_says_expired_until_its_grace_ends_and_fencing_survives() {
    let fake = Fake::start();
    let backend = backend(&fake);
    let grant = backend
        .acquire(
            &target("vms/one"),
            &AcquireRequest::new(Duration::from_secs(30)),
        )
        .unwrap();
    fake.advance_clock(Duration::from_secs(60));
    assert!(backend.get(&target("vms/one")).unwrap().is_none());
    assert_eq!(
        backend
            .renew(&grant.renewal_token, Duration::from_secs(30))
            .unwrap_err()
            .code(),
        "expired"
    );
    // The grace window has passed: the lapsed hash is gone, so the token is just unknown.
    let again = backend.acquire(&target("vms/one"), &lease(30)).unwrap();
    fake.advance_clock(Duration::from_secs(2 * 3600));
    assert_eq!(
        backend
            .renew(&again.renewal_token, Duration::from_secs(30))
            .unwrap_err()
            .code(),
        "token_invalid"
    );
    // The counter outlives all of it, so a later grant still outranks every earlier one.
    let later = backend.acquire(&target("vms/one"), &lease(30)).unwrap();
    assert!(later.claim.fencing_token > again.claim.fencing_token);
    assert!(again.claim.fencing_token > grant.claim.fencing_token);
}

// ---- waiting ------------------------------------------------------------------------

#[test]
fn hs2_k9j1rk_a_wait_holds_one_stream_and_is_woken_by_the_release() {
    let fake = Fake::start();
    let backend = backend(&fake);
    let held = backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    let grant = std::thread::scope(|scope| {
        let waiter = scope.spawn(|| {
            backend.acquire_wait(&target("vms/one"), &lease(60), Duration::from_secs(20))
        });
        std::thread::sleep(Duration::from_millis(500));
        backend.release(&held.renewal_token).unwrap();
        waiter.join().unwrap()
    })
    .unwrap();
    assert!(grant.claim.fencing_token > held.claim.fencing_token);
    assert_eq!(fake.count("subscribe"), 1, "one stream for the whole wait");
}

#[test]
fn hs2_k9j1rk_a_parked_wait_sends_no_requests_while_it_is_idle() {
    let fake = Fake::start();
    let backend = backend(&fake);
    let held = backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    std::thread::scope(|scope| {
        let waiter =
            scope.spawn(|| backend.wait_free(&target("vms/one"), Duration::from_millis(1500)));
        std::thread::sleep(Duration::from_millis(400));
        let early = fake.requests();
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(
            fake.requests(),
            early,
            "no request while parked: {:?}",
            fake.log.lock().unwrap()
        );
        match waiter.join().unwrap().unwrap() {
            WaitOutcome::TimedOut { holder } => assert_eq!(holder.claim_id, held.claim.claim_id),
            other => panic!("{other:?}"),
        }
    });
    assert_eq!(fake.count("subscribe"), 1);
}

#[test]
fn hs2_k9j1rk_expiry_wakes_a_wait_with_one_timeout_and_no_message() {
    let fake = Fake::start();
    let backend = backend(&fake);
    backend
        .acquire(
            &target("vms/one"),
            &AcquireRequest::new(Duration::from_millis(900)),
        )
        .unwrap();
    let started = Instant::now();
    let outcome = backend
        .wait_free(&target("vms/one"), Duration::from_secs(20))
        .unwrap();
    assert_eq!(outcome, WaitOutcome::Free);
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(700) && waited < Duration::from_secs(5),
        "{waited:?}"
    );
    assert_eq!(
        fake.count("subscribe"),
        1,
        "one sleep to the instant Redis reported"
    );
}

#[test]
fn hs2_k9j1rk_a_release_that_lands_before_the_subscription_is_confirmed_is_not_missed() {
    let fake = Fake::start();
    let backend = backend(&fake);
    backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    // Between the first read and the stream's confirmation the claim disappears without a
    // message (the lost wake-up a push-only design would hang on).
    *fake.faults.on_subscribe.lock().unwrap() = Some(Box::new(|fake| {
        fake.delete_silently("hs2rc:v1:vms:c:one");
    }));
    let started = Instant::now();
    assert_eq!(
        backend
            .wait_free(&target("vms/one"), Duration::from_secs(20))
            .unwrap(),
        WaitOutcome::Free
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn hs2_k9j1rk_failed_streams_back_off_and_resubscribe_to_the_same_release() {
    let fake = Fake::start();
    let backend = backend(&fake);
    let held = backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    fake.faults
        .fail_subscribe
        .store(1, std::sync::atomic::Ordering::SeqCst);
    fake.faults
        .close_after_confirm
        .store(1, std::sync::atomic::Ordering::SeqCst);
    let outcome = std::thread::scope(|scope| {
        let waiter = scope.spawn(|| backend.wait_free(&target("vms/one"), Duration::from_secs(30)));
        std::thread::sleep(Duration::from_millis(1500));
        backend.release(&held.renewal_token).unwrap();
        waiter.join().unwrap()
    })
    .unwrap();
    assert_eq!(outcome, WaitOutcome::Free);
    assert!(
        fake.count("subscribe") >= 3,
        "one 500, one closed stream, one that delivered"
    );
}

#[test]
fn hs2_k9j1rk_a_stream_that_keeps_failing_gives_up_instead_of_looping() {
    let fake = Fake::start();
    let backend = backend(&fake);
    backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    fake.faults
        .fail_subscribe
        .store(100, std::sync::atomic::Ordering::SeqCst);
    let started = Instant::now();
    let error = backend
        .wait_free(&target("vms/one"), Duration::from_secs(120))
        .unwrap_err();
    assert_eq!(error.code(), "backend_unreachable");
    assert!(
        error.to_string().contains("subscription kept failing"),
        "{error}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(40),
        "bounded backoff: {:?}",
        started.elapsed()
    );
    assert!(fake.count("subscribe") <= 8);
}

// ---- listing and namespaces ---------------------------------------------------------

#[test]
fn hs2_k9j1rk_list_reads_the_resource_set_with_one_pipeline() {
    let fake = Fake::start();
    let backend = backend(&fake);
    let a = backend.acquire(&target("vms/a"), &lease(60)).unwrap();
    let b = backend.acquire(&target("vms/b"), &lease(60)).unwrap();
    backend.acquire(&target("other/z"), &lease(60)).unwrap();
    backend.release(&a.renewal_token).unwrap();
    let before = fake.count("pipeline");
    let listed = backend.list(&Namespace::parse("vms").unwrap()).unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|c| c.resource.as_str())
            .collect::<Vec<_>>(),
        ["b"]
    );
    assert_eq!(listed[0].claim_id, b.claim.claim_id);
    assert_eq!(
        fake.count("pipeline") - before,
        1,
        "one round trip for every resource"
    );
    assert!(
        backend
            .list(&Namespace::parse("empty").unwrap())
            .unwrap()
            .is_empty()
    );
    let names = backend.namespaces().unwrap();
    assert_eq!(
        names.iter().map(Namespace::as_str).collect::<Vec<_>>(),
        ["other", "vms"]
    );
}

// ---- configuration ------------------------------------------------------------------

#[derive(Default)]
struct Memory(std::sync::Mutex<std::collections::BTreeMap<String, String>>);

impl SecretStore for Memory {
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        self.0.lock().unwrap().insert(account.into(), secret.into());
        Ok(())
    }
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }
    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        Ok(self.0.lock().unwrap().remove(account).is_some())
    }
}

fn configuration(url: &str) -> BackendConfiguration {
    let mut c = BackendConfiguration::new("up", BackendKind::UpstashRedis).unwrap();
    c.settings.insert("url".into(), url.into());
    c
}

#[test]
fn hs2_k9j1rk_the_token_comes_from_the_credential_store_and_a_missing_one_names_the_command() {
    let fake = Fake::start();
    let secrets = Memory::default();
    let shared = SharedSecrets(Arc::new(Memory::default()));
    let missing = upstash::backend_with(&configuration(&fake.url), shared, |u, t| {
        Arc::new(UreqTransport::new(u, t))
    })
    .err()
    .expect("no token yet");
    assert_eq!(missing.code(), "unsupported");
    assert!(
        missing
            .to_string()
            .contains("resource backend secret up token"),
        "{missing}"
    );

    secrets.set("resource-claims.up.token", TOKEN).unwrap();
    let ready = upstash::backend_with(
        &configuration(&fake.url),
        SharedSecrets(Arc::new(secrets)),
        |u, t| Arc::new(UreqTransport::new(u, t)),
    )
    .unwrap();
    let grant = ready.acquire(&target("vms/one"), &lease(60)).unwrap();
    assert_eq!(
        ready.get(&target("vms/one")).unwrap().unwrap().claim_id,
        grant.claim.claim_id
    );
}

#[test]
fn hs2_k9j1rk_urls_and_prefixes_are_checked_before_anything_is_stored() {
    for good in [
        "https://example-db.upstash.io",
        "http://127.0.0.1:8080",
        "http://localhost:1",
    ] {
        assert!(validate_url(good).is_ok(), "{good}");
    }
    for bad in [
        "http://example-db.upstash.io",
        "https://user:pw@example.io",
        "https://example.io/?_token=x",
        "https://example.io/#frag",
        "https://",
        "ftp://example.io",
        "https://exa mple.io",
    ] {
        assert!(validate_url(bad).is_err(), "{bad}");
    }
    let mut c = configuration("https://example-db.upstash.io");
    assert_eq!(upstash::check_settings(&c).unwrap().1, "hs2rc");
    c.settings.insert("prefix".into(), "Team:One".into());
    assert!(upstash::check_settings(&c).is_err());
    c.settings.insert("prefix".into(), "team-one".into());
    assert_eq!(upstash::check_settings(&c).unwrap().1, "team-one");
    let mut none = BackendConfiguration::new("up", BackendKind::UpstashRedis);
    assert!(none.as_mut().is_err() || none.unwrap().validate().is_err());
}
