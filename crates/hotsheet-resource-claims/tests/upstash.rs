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
    AcquireRequest, ClaimBackend, Namespace, ResourceRef, WaitCancel, WaitMechanism, WaitOutcome,
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
        .renew(grant.renewal_token.expose(), Duration::from_secs(90))
        .unwrap();
    backend.release(grant.renewal_token.expose()).unwrap();
    for key in fake.keys() {
        assert!(key.starts_with("hs2rc:v1:"), "{key}");
        assert!(!key.contains("hsrc1"), "{key}");
    }
    let evals = fake.evals.lock().unwrap().clone();
    assert!(evals.len() >= 3);
    for eval in &evals {
        let wire = eval.to_string();
        assert!(!wire.contains(grant.renewal_token.expose()), "{wire}");
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
            .renew(grant.renewal_token.expose(), Duration::from_secs(30))
            .unwrap_err()
            .code(),
        "expired"
    );
    // The grace window has passed: the lapsed hash is gone, so the token is just unknown.
    let again = backend.acquire(&target("vms/one"), &lease(30)).unwrap();
    fake.advance_clock(Duration::from_secs(2 * 3600));
    assert_eq!(
        backend
            .renew(again.renewal_token.expose(), Duration::from_secs(30))
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
        backend.release(held.renewal_token.expose()).unwrap();
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

/// HS2-QX9QQE: a stream whose read timeout ends early, as a Windows ARM64 socket timer does.
struct EarlyTimeout(UreqTransport);

struct EarlyStream(Box<dyn upstash::transport::Subscription>, bool);

impl upstash::transport::Subscription for EarlyStream {
    fn next(&mut self) -> Result<upstash::transport::Event, upstash::transport::RestError> {
        // The confirmation is read; the next read "times out" at once, long before the
        // lifetime the backend asked for.
        if self.1 {
            return Ok(upstash::transport::Event::Timeout);
        }
        self.1 = true;
        self.0.next()
    }
}

impl upstash::transport::RestTransport for EarlyTimeout {
    fn command(
        &self,
        args: &[serde_json::Value],
    ) -> Result<serde_json::Value, upstash::transport::RestError> {
        self.0.command(args)
    }
    fn pipeline(
        &self,
        commands: &[Vec<serde_json::Value>],
    ) -> Result<Vec<serde_json::Value>, upstash::transport::RestError> {
        self.0.pipeline(commands)
    }
    fn subscribe(
        &self,
        channel: &str,
        lifetime: Duration,
    ) -> Result<Box<dyn upstash::transport::Subscription>, upstash::transport::RestError> {
        Ok(Box::new(EarlyStream(
            self.0.subscribe(channel, lifetime)?,
            false,
        )))
    }
}

#[test]
fn hs2_qx9qqe_an_early_stream_timeout_waits_out_its_lifetime_without_resubscribing() {
    let fake = Fake::start();
    let backend = UpstashBackend::new(
        "up",
        "hs2rc",
        Arc::new(EarlyTimeout(UreqTransport::new(&fake.url, TOKEN))),
        None,
    );
    let held = backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    let started = Instant::now();
    match backend
        .wait_free(&target("vms/one"), Duration::from_millis(600))
        .unwrap()
    {
        WaitOutcome::TimedOut { holder } => assert_eq!(holder.claim_id, held.claim.claim_id),
        other => panic!("{other:?}"),
    }
    assert!(started.elapsed() >= Duration::from_millis(600));
    assert_eq!(
        fake.count("subscribe"),
        1,
        "an early timeout is not a reason to subscribe again: {:?}",
        fake.log.lock().unwrap()
    );
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
        backend.release(held.renewal_token.expose()).unwrap();
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

// ---- HS2-P234BH: a cancel during the wait ---------------------------------------------

/// A generous bound for "returned promptly" on a loaded machine; every wait below would
/// otherwise last at least a minute.
const PROMPT: Duration = Duration::from_secs(5);

/// The real transport, telling the test when each stream it opened is dropped.
struct Tracked(UreqTransport, std::sync::mpsc::Sender<()>);

struct TrackedStream(
    Box<dyn upstash::transport::Subscription>,
    std::sync::mpsc::Sender<()>,
);

impl Drop for TrackedStream {
    fn drop(&mut self) {
        let _ = self.1.send(());
    }
}

impl upstash::transport::Subscription for TrackedStream {
    fn next(&mut self) -> Result<upstash::transport::Event, upstash::transport::RestError> {
        self.0.next()
    }

    fn closer(&self) -> Option<upstash::transport::StreamCloser> {
        self.0.closer()
    }
}

impl upstash::transport::RestTransport for Tracked {
    fn command(
        &self,
        args: &[serde_json::Value],
    ) -> Result<serde_json::Value, upstash::transport::RestError> {
        self.0.command(args)
    }
    fn pipeline(
        &self,
        commands: &[Vec<serde_json::Value>],
    ) -> Result<Vec<serde_json::Value>, upstash::transport::RestError> {
        self.0.pipeline(commands)
    }
    fn subscribe(
        &self,
        channel: &str,
        lifetime: Duration,
    ) -> Result<Box<dyn upstash::transport::Subscription>, upstash::transport::RestError> {
        Ok(Box::new(TrackedStream(
            self.0.subscribe(channel, lifetime)?,
            self.1.clone(),
        )))
    }
}

fn tracked(fake: &Fake) -> (Arc<UpstashBackend>, std::sync::mpsc::Receiver<()>) {
    let (dropped, streams_ended) = std::sync::mpsc::channel();
    let backend = UpstashBackend::new(
        "up",
        "hs2rc",
        Arc::new(Tracked(UreqTransport::new(&fake.url, TOKEN), dropped)),
        None,
    );
    (Arc::new(backend), streams_ended)
}

/// Start a cancellable wait on its own thread; its result arrives on the receiver.
fn spawn_wait(
    backend: &Arc<UpstashBackend>,
    cancel: &WaitCancel,
    wait: Duration,
) -> std::sync::mpsc::Receiver<Result<WaitOutcome, hotsheet_resource_claims::ClaimError>> {
    let (done, result) = std::sync::mpsc::channel();
    let (backend, cancel) = (backend.clone(), cancel.clone());
    std::thread::spawn(move || {
        let _ = done.send(backend.wait_free_cancellable(&target("vms/one"), wait, &cancel));
    });
    result
}

/// Block until the fake has logged `count` requests starting with `prefix`.
fn until_logged(fake: &Fake, prefix: &str, count: usize) {
    let started = Instant::now();
    while fake.count(prefix) < count {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "never saw {count} x {prefix}: {:?}",
            fake.log.lock().unwrap()
        );
        std::thread::yield_now();
    }
}

fn log_since(fake: &Fake, from: usize) -> Vec<String> {
    fake.log.lock().unwrap()[from..].to_vec()
}

#[test]
fn hs2_p234bh_a_cancel_ends_a_parked_stream_wait_and_its_stream_reads_nothing_after() {
    let fake = Fake::start();
    let (backend, streams_ended) = tracked(&fake);
    let held = backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    let cancel = WaitCancel::new();
    let parked = spawn_wait(&backend, &cancel, Duration::from_secs(120));
    // Parked: the first read, the stream, and the read once the stream was confirmed.
    until_logged(&fake, "eval get", 2);
    until_logged(&fake, "subscribe", 1);
    // The wait's waker and the stream's closer (HS2-VDF7VS).
    until_registered(&cancel, 2);
    let from = fake.requests();

    let began = Instant::now();
    cancel.cancel();
    let error = parked.recv_timeout(PROMPT).unwrap().unwrap_err();
    assert!(began.elapsed() < PROMPT, "{:?}", began.elapsed());
    assert_eq!(error.code(), "interrupted");
    // The waker is removed as the wait ends; the stream's closer as its helper thread
    // drops the closed stream (HS2-VDF7VS).
    until_registered(&cancel, 0);

    // The abandoned stream ends (the cancel closes it, HS2-VDF7VS) without reading state or
    // subscribing again: the only request after the cancel is the release itself.
    backend.release(held.renewal_token.expose()).unwrap();
    streams_ended.recv_timeout(PROMPT).unwrap();
    assert_eq!(log_since(&fake, from), ["eval release"]);
    assert!(
        backend.get(&target("vms/one")).unwrap().is_none(),
        "nothing claimed"
    );

    // Cancelled before it starts (and again): no request at all.
    let from = fake.requests();
    for _ in 0..2 {
        let error = backend
            .wait_free_cancellable(&target("vms/one"), Duration::from_secs(120), &cancel)
            .unwrap_err();
        assert_eq!(error.code(), "interrupted");
    }
    assert_eq!(fake.requests(), from);

    // A fresh signal waits again and a release still wakes it.
    let held = backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    let subscribed = fake.count("subscribe");
    let again = spawn_wait(&backend, &WaitCancel::new(), Duration::from_secs(120));
    until_logged(&fake, "subscribe", subscribed + 1);
    backend.release(held.renewal_token.expose()).unwrap();
    assert_eq!(
        again.recv_timeout(PROMPT).unwrap().unwrap(),
        WaitOutcome::Free
    );
}

#[test]
fn hs2_p234bh_an_abandoned_stream_ends_at_its_lifetime_without_a_request() {
    let fake = Fake::start();
    let (backend, streams_ended) = tracked(&fake);
    // A short lease: the stream lives to its expiry, which would otherwise wake the wait.
    backend
        .acquire(
            &target("vms/one"),
            &AcquireRequest::new(Duration::from_millis(1500)),
        )
        .unwrap();
    let cancel = WaitCancel::new();
    let parked = spawn_wait(&backend, &cancel, Duration::from_secs(120));
    until_logged(&fake, "eval get", 2);
    let from = fake.requests();
    cancel.cancel();
    assert_eq!(
        parked.recv_timeout(PROMPT).unwrap().unwrap_err().code(),
        "interrupted"
    );
    // The stream is dropped (closed by the cancel, HS2-VDF7VS, before its lifetime);
    // nothing reads the expiry.
    streams_ended.recv_timeout(PROMPT).unwrap();
    assert!(
        log_since(&fake, from).is_empty(),
        "{:?}",
        log_since(&fake, from)
    );
}

#[test]
fn hs2_p234bh_a_cancel_during_the_backoff_after_a_failed_stream_ends_the_wait() {
    let fake = Fake::start();
    let backend = Arc::new(backend(&fake));
    backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    fake.faults
        .fail_subscribe
        .store(100, std::sync::atomic::Ordering::SeqCst);
    let cancel = WaitCancel::new();
    let parked = spawn_wait(&backend, &cancel, Duration::from_secs(120));
    // After the fourth failed stream the backoff is 2 s.
    until_logged(&fake, "subscribe", 4);
    let began = Instant::now();
    cancel.cancel();
    assert_eq!(
        parked.recv_timeout(PROMPT).unwrap().unwrap_err().code(),
        "interrupted"
    );
    assert!(
        began.elapsed() < Duration::from_millis(1500),
        "the backoff was not waited out: {:?}",
        began.elapsed()
    );
}

// ---- HS2-VDF7VS: a cancel closes the abandoned stream ----------------------------------

/// Block until the fake has seen the client close `count` streams; how long that took.
fn until_closed_by_client(fake: &Fake, count: usize) -> Duration {
    let started = Instant::now();
    while fake.closed_by_client() < count {
        assert!(
            started.elapsed() < PROMPT,
            "the fake saw {} closed streams, wanted {count}",
            fake.closed_by_client()
        );
        std::thread::yield_now();
    }
    started.elapsed()
}

/// Block until `cancel` has `count` registrations (the wait's waker, the stream's closer).
fn until_registered(cancel: &WaitCancel, count: usize) {
    let started = Instant::now();
    while cancel.registered() != count {
        assert!(
            started.elapsed() < PROMPT,
            "{} registrations, wanted {count}",
            cancel.registered()
        );
        std::thread::yield_now();
    }
}

#[test]
fn hs2_vdf7vs_a_cancel_closes_the_abandoned_stream_at_once() {
    let fake = Fake::start();
    let (backend, streams_ended) = tracked(&fake);
    let held = backend.acquire(&target("vms/one"), &lease(600)).unwrap();
    for round in 1..=2 {
        let cancel = WaitCancel::new();
        let parked = spawn_wait(&backend, &cancel, Duration::from_secs(600));
        // Parked on its stream, with the stream's closer registered beside the waker.
        until_logged(&fake, "eval get", 2 * round);
        until_registered(&cancel, 2);
        assert_eq!(
            fake.closed_by_client(),
            round - 1,
            "the stream is still open"
        );
        let from = fake.requests();

        cancel.cancel();
        assert_eq!(
            parked.recv_timeout(PROMPT).unwrap().unwrap_err().code(),
            "interrupted"
        );
        // The fake sees the stream close right after the cancel, not at the holder's
        // lease (ten minutes away), and nothing is sent after it.
        let took = until_closed_by_client(&fake, round);
        assert!(took < PROMPT, "{took:?}");
        streams_ended.recv_timeout(PROMPT).unwrap();
        until_registered(&cancel, 0);
        assert!(
            log_since(&fake, from).is_empty(),
            "{:?}",
            log_since(&fake, from)
        );
    }

    // A wait nobody cancels keeps its stream open until the release wakes it.
    let subscribed = fake.count("subscribe");
    let waiting = spawn_wait(&backend, &WaitCancel::new(), Duration::from_secs(600));
    until_logged(&fake, "subscribe", subscribed + 1);
    until_logged(&fake, "eval get", 5);
    assert_eq!(fake.closed_by_client(), 2);
    backend.release(held.renewal_token.expose()).unwrap();
    assert_eq!(
        waiting.recv_timeout(PROMPT).unwrap().unwrap(),
        WaitOutcome::Free
    );
}

#[test]
fn hs2_vdf7vs_a_cancel_before_the_closer_is_registered_still_closes_the_stream() {
    let fake = Fake::start();
    let backend = Arc::new(backend(&fake));
    backend.acquire(&target("vms/one"), &lease(600)).unwrap();
    let cancel = WaitCancel::new();
    // The cancel lands while the subscribe is in flight: after the fake registered the
    // stream and before it answered, so before the client could register the closer.
    let cancelling = cancel.clone();
    *fake.faults.on_subscribe.lock().unwrap() = Some(Box::new(move |_| cancelling.cancel()));
    let parked = spawn_wait(&backend, &cancel, Duration::from_secs(600));
    assert_eq!(
        parked.recv_timeout(PROMPT).unwrap().unwrap_err().code(),
        "interrupted"
    );
    let took = until_closed_by_client(&fake, 1);
    assert!(took < PROMPT, "{took:?}");
    until_registered(&cancel, 0);
    // The stream was dropped unread: no read after the subscribe.
    let log = fake.log.lock().unwrap().clone();
    assert!(
        log.last()
            .is_some_and(|last| last.starts_with("subscribe ")),
        "{log:?}"
    );
}

// ---- HS2-4XC2Y3: the stream closer itself, on every platform ---------------------------

#[test]
fn hs2_4xc2y3_the_stream_closer_ends_only_its_own_blocked_read() {
    use upstash::transport::{Event, RestTransport};

    let fake = Fake::start();
    let transport = UreqTransport::new(&fake.url, TOKEN);
    let (mut first, mut second) = (
        transport
            .subscribe("synthetic-a", Duration::from_secs(600))
            .unwrap(),
        transport
            .subscribe("synthetic-b", Duration::from_secs(600))
            .unwrap(),
    );
    assert_eq!(first.next().unwrap(), Event::Subscribed);
    assert_eq!(second.next().unwrap(), Event::Subscribed);
    let close = first
        .closer()
        .expect("the real transport can close a stream");

    // A reader blocked on the first stream (nothing will ever be published to it).
    let (read, ended) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = read.send(first.next());
    });
    let began = Instant::now();
    close();
    let outcome = ended.recv_timeout(PROMPT).expect("the blocked read ended");
    assert!(began.elapsed() < PROMPT, "{:?}", began.elapsed());
    assert!(matches!(outcome, Ok(Event::Closed) | Err(_)), "{outcome:?}");
    until_closed_by_client(&fake, 1);
    // Closing again finds nothing left to close.
    close();

    // The other stream, on its own agent, is still open and still delivers.
    assert_eq!(
        transport
            .command(&[
                serde_json::json!("PUBLISH"),
                serde_json::json!("synthetic-b"),
                serde_json::json!("hello")
            ])
            .unwrap(),
        serde_json::json!(1)
    );
    assert_eq!(second.next().unwrap(), Event::Message("hello".into()));
    assert_eq!(fake.closed_by_client(), 1);
}

// ---- listing and namespaces ---------------------------------------------------------

#[test]
fn hs2_k9j1rk_list_reads_the_resource_set_with_one_pipeline() {
    let fake = Fake::start();
    let backend = backend(&fake);
    let a = backend.acquire(&target("vms/a"), &lease(60)).unwrap();
    let b = backend.acquire(&target("vms/b"), &lease(60)).unwrap();
    backend.acquire(&target("other/z"), &lease(60)).unwrap();
    backend.release(a.renewal_token.expose()).unwrap();
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
