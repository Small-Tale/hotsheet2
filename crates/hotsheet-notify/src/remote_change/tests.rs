//! HS2-8X2S08: the remote-change router, registry, workflow and hub, against a scripted fake
//! adapter and an in-memory credential store. Synthetic names only; no network, no keychain.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use hotsheet_ticketing::claim_backends::Change;
use hotsheet_ticketing::secrets::{SecretError, SecretStore};

use super::hub::{Backoff, ChangeListener, Hub, Ignored, RecheckReason, RemoteChange, Router};
use super::model::*;
use super::registry::*;
use super::workflow::Workflow;

// ---- fixtures -----------------------------------------------------------------------

#[derive(Default)]
struct Memory(Mutex<BTreeMap<String, String>>);

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

fn repo(name: &str) -> RepoIdentity {
    RepoIdentity {
        id: None,
        full_name: Some(name.into()),
    }
}

fn hint(name: &str, reference: &str, tip: &str, event: &str) -> ChangeHint {
    ChangeHint {
        repository: repo(name),
        ref_name: reference.into(),
        advertised_tip: Some(tip.into()),
        event_id: Some(event.into()),
        at_ms: 1,
    }
}

fn binding(source: &str, connection: &str, channel: &str, name: &str) -> Binding {
    Binding {
        source: source.into(),
        connection: connection.into(),
        channel: channel.into(),
        repository: repo(name),
        refs: vec!["refs/heads/main".into()],
    }
}

const TIP_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TIP_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const MAIN: &str = "refs/heads/main";

// ---- the router ---------------------------------------------------------------------

fn router(window: u64) -> Router {
    Router::new(
        vec![
            binding("checkout:code", "ably1", "ch-code", "acme/app"),
            binding("store:tickets", "ably1", "ch-store", "acme/tickets"),
        ],
        window,
    )
}

#[test]
fn hs2_8x2s08_a_hint_for_an_unknown_channel_repository_or_ref_is_ignored_and_counted() {
    let mut r = router(0);
    let ok = hint("acme/app", MAIN, TIP_A, "e1");
    assert_eq!(
        r.offer("ably1", "other", &ok, 1),
        Err(Ignored::UnknownChannel)
    );
    assert_eq!(
        r.offer("nope", "ch-code", &ok, 1),
        Err(Ignored::UnknownChannel)
    );
    let wrong = hint("evil/app", MAIN, TIP_A, "e2");
    assert_eq!(
        r.offer("ably1", "ch-code", &wrong, 1),
        Err(Ignored::WrongRepository)
    );
    let tag = hint("acme/app", "refs/tags/v1", TIP_A, "e3");
    assert_eq!(
        r.offer("ably1", "ch-code", &tag, 1),
        Err(Ignored::UnfollowedRef)
    );
    assert_eq!(r.ignored["unknown_channel"], 2);
    assert_eq!(r.ignored["wrong_repository"], 1);
    assert_eq!(r.ignored["unfollowed_ref"], 1);
    // A hint that names no repository can never match a binding.
    let nameless = ChangeHint {
        repository: RepoIdentity::default(),
        ..ok
    };
    assert_eq!(
        r.offer("ably1", "ch-code", &nameless, 1),
        Err(Ignored::WrongRepository)
    );
}

#[test]
fn hs2_8x2s08_repository_identity_compares_ids_and_names_and_never_trusts_a_mismatch() {
    let by_id = RepoIdentity {
        id: Some(7),
        full_name: None,
    };
    let both = RepoIdentity {
        id: Some(7),
        full_name: Some("Acme/App".into()),
    };
    assert!(by_id.matches(&both), "an id both sides state agrees");
    assert!(
        both.matches(&repo("acme/app")),
        "names compare case-insensitively"
    );
    assert!(
        !RepoIdentity {
            id: Some(8),
            full_name: Some("acme/app".into())
        }
        .matches(&both),
        "a differing id wins over a matching name"
    );
    assert!(
        !RepoIdentity {
            id: Some(7),
            full_name: Some("other/app".into())
        }
        .matches(&both),
        "a differing name loses"
    );
    assert!(
        !by_id.matches(&repo("acme/app")),
        "nothing in common is not a match"
    );
}

#[test]
fn hs2_8x2s08_a_ref_filter_is_an_exact_name_or_a_trailing_wildcard() {
    let mut b = binding("checkout:c", "x", "c", "a/b");
    b.refs = vec!["refs/heads/release/*".into(), "refs/heads/main".into()];
    assert!(b.follows("refs/heads/main"));
    assert!(b.follows("refs/heads/release/1.2"));
    assert!(!b.follows("refs/heads/mainline"));
    assert!(!b.follows("refs/tags/release/1"));
}

#[test]
fn hs2_8x2s08_duplicates_collapse_by_event_id_and_by_ref_and_tip() {
    let mut r = router(0);
    let first = hint("acme/app", MAIN, TIP_A, "e1");
    assert_eq!(r.offer("ably1", "ch-code", &first, 1).unwrap().len(), 1);
    // The same delivery again, and the same push under a new delivery id.
    assert_eq!(
        r.offer("ably1", "ch-code", &first, 2),
        Err(Ignored::Duplicate)
    );
    let renamed = hint("acme/app", MAIN, TIP_A, "e2");
    assert_eq!(
        r.offer("ably1", "ch-code", &renamed, 3),
        Err(Ignored::Duplicate)
    );
    // A new tip is new work; an older tip arriving late is a different tip, so it is a hint
    // too (a check is idempotent), but its repeat is a duplicate.
    assert_eq!(
        r.offer("ably1", "ch-code", &hint("acme/app", MAIN, TIP_B, "e3"), 4)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        r.offer("ably1", "ch-code", &hint("acme/app", MAIN, TIP_A, "e4"), 5),
        Err(Ignored::Duplicate)
    );
    assert_eq!(r.ignored["duplicate"], 3);
}

#[test]
fn hs2_8x2s08_sources_sharing_a_channel_each_get_one_delivery_and_a_source_on_two_channels_one() {
    let mut r = Router::new(
        vec![
            binding("checkout:a", "c", "shared", "acme/app"),
            binding("store:t", "c", "shared", "acme/app"),
            binding("checkout:a", "c", "second", "acme/app"),
        ],
        0,
    );
    let on_shared = hint("acme/app", MAIN, TIP_A, "e1");
    let delivered = r.offer("c", "shared", &on_shared, 1).unwrap();
    let mut sources: Vec<&str> = delivered.iter().map(|d| d.source.as_str()).collect();
    sources.sort_unstable();
    assert_eq!(sources, ["checkout:a", "store:t"]);
    // The same event arriving on the source's second channel is not processed twice.
    assert_eq!(
        r.offer("c", "second", &on_shared, 2),
        Err(Ignored::Duplicate)
    );
}

#[test]
fn hs2_8x2s08_a_burst_is_coalesced_to_its_leading_hint_and_one_trailing_hint() {
    let mut r = router(1000);
    let take = |r: &mut Router, tip: &str, event: &str, at: u64| {
        r.offer("ably1", "ch-code", &hint("acme/app", MAIN, tip, event), at)
            .unwrap()
    };
    assert_eq!(
        take(&mut r, TIP_A, "e1", 0).len(),
        1,
        "the leading edge goes out at once"
    );
    assert!(
        take(&mut r, TIP_B, "e2", 100).is_empty(),
        "inside the window it is held"
    );
    let c = "cccccccccccccccccccccccccccccccccccccccc";
    assert!(take(&mut r, c, "e3", 200).is_empty());
    assert_eq!(r.next_due(), Some(1000));
    assert!(r.due(999).is_empty());
    let trailing = r.due(1000);
    assert_eq!(
        trailing.len(),
        1,
        "one trailing delivery for the whole burst"
    );
    assert_eq!(
        trailing[0].advertised_tip.as_deref(),
        Some(c),
        "the latest hint wins"
    );
    assert_eq!(r.next_due(), None);
    // After a quiet window the next hint is a leading edge again.
    let d = "dddddddddddddddddddddddddddddddddddddddd";
    assert_eq!(take(&mut r, d, "e4", 5000).len(), 1);
}

#[test]
fn hs2_8x2s08_unbinding_a_source_forgets_what_was_held_for_it() {
    let mut r = router(1000);
    r.offer("ably1", "ch-code", &hint("acme/app", MAIN, TIP_A, "e1"), 0)
        .unwrap();
    r.offer("ably1", "ch-code", &hint("acme/app", MAIN, TIP_B, "e2"), 10)
        .unwrap();
    assert!(r.next_due().is_some());
    r.forget_source("checkout:code");
    assert!(r.next_due().is_none());
    assert!(r.due(10_000).is_empty());
}

#[test]
fn hs2_8x2s08_the_duplicate_memory_is_bounded() {
    let mut r = router(0);
    for n in 0..5000 {
        let _ = r.offer(
            "ably1",
            "ch-code",
            &hint("acme/app", MAIN, &format!("{n:040x}"), &format!("e{n}")),
            n,
        );
    }
    let again = hint("acme/app", MAIN, &format!("{:040x}", 0), "e0");
    assert!(
        r.offer("ably1", "ch-code", &again, 9999).is_ok(),
        "the oldest entries were forgotten, not kept forever"
    );
}

// ---- the registry -------------------------------------------------------------------

fn home() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn hs2_8x2s08_the_registry_is_idempotent_and_refuses_a_bad_binding() {
    let dir = home();
    let conn = || Connection::new("ably1", "ably").unwrap();
    assert_eq!(
        Registry::update(dir.path(), |r| r.upsert_connection(conn())).unwrap(),
        Change::Created
    );
    assert_eq!(
        Registry::update(dir.path(), |r| r.upsert_connection(conn())).unwrap(),
        Change::Unchanged
    );
    let b = binding("checkout:code", "ably1", "ch", "acme/app");
    assert_eq!(
        Registry::update(dir.path(), |r| r.bind(b.clone())).unwrap(),
        Change::Created
    );
    assert_eq!(
        Registry::update(dir.path(), |r| r.bind(b.clone())).unwrap(),
        Change::Unchanged
    );
    let mut changed = b.clone();
    changed.refs = vec!["refs/heads/*".into()];
    assert_eq!(
        Registry::update(dir.path(), |r| r.bind(changed)).unwrap(),
        Change::Updated
    );

    for (mutate, needle) in [
        (
            Box::new(|b: &mut Binding| b.source = "elsewhere".into()) as Box<dyn Fn(&mut Binding)>,
            "store:<instance id>",
        ),
        (
            Box::new(|b: &mut Binding| b.repository = RepoIdentity::default()),
            "expects",
        ),
        (
            Box::new(|b: &mut Binding| b.repository = repo("not-a-repo")),
            "owner/name",
        ),
        (
            Box::new(|b: &mut Binding| b.refs = vec!["main".into()]),
            "start with refs/",
        ),
        (Box::new(|b: &mut Binding| b.refs = vec![]), "1-16 refs"),
        (
            Box::new(|b: &mut Binding| b.channel = "has space".into()),
            "channel",
        ),
        (
            Box::new(|b: &mut Binding| b.connection = "ghost".into()),
            "no connection 'ghost'",
        ),
    ] {
        let mut bad = b.clone();
        mutate(&mut bad);
        let error = Registry::update(dir.path(), |r| r.bind(bad))
            .unwrap_err()
            .to_string();
        assert!(error.contains(needle), "{needle}: {error}");
    }
}

#[test]
fn hs2_8x2s08_removing_a_bound_connection_is_refused_until_it_is_unbound() {
    let dir = home();
    Registry::update(dir.path(), |r| {
        r.upsert_connection(Connection::new("ably1", "ably").unwrap())
    })
    .unwrap();
    Registry::update(dir.path(), |r| {
        r.bind(binding("checkout:code", "ably1", "ch", "acme/app"))
    })
    .unwrap();
    let refused = Registry::update(dir.path(), |r| r.remove_connection("ably1")).unwrap_err();
    assert_eq!(refused.code(), "conflict");
    assert!(
        refused.to_string().contains("checkout:code on ch"),
        "{refused}"
    );
    assert_eq!(
        Registry::update(dir.path(), |r| Ok(r.unbind("checkout:code", "ably1", "ch"))).unwrap(),
        Change::Removed
    );
    assert_eq!(
        Registry::update(dir.path(), |r| Ok(r.unbind("checkout:code", "ably1", "ch"))).unwrap(),
        Change::Absent
    );
    assert_eq!(
        Registry::update(dir.path(), |r| r.remove_connection("ably1")).unwrap(),
        Change::Removed
    );
    assert_eq!(
        Registry::update(dir.path(), |r| r.remove_connection("ably1")).unwrap(),
        Change::Absent
    );
}

#[test]
fn hs2_8x2s08_one_subscription_serves_every_source_on_a_channel() {
    let mut r = Registry::default();
    r.upsert_connection(Connection::new("c", "ably").unwrap())
        .unwrap();
    for (source, channel) in [
        ("checkout:a", "one"),
        ("store:b", "one"),
        ("store:c", "two"),
    ] {
        r.bind(binding(source, "c", channel, "acme/app")).unwrap();
    }
    assert_eq!(r.subscriptions()["c"], ["one", "two"]);
}

#[test]
fn hs2_8x2s08_a_newer_or_damaged_registry_is_refused_and_left_alone() {
    let dir = home();
    let path = registry_path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    for text in [r#"{"$hotsheetSchema": 99}"#, "not json"] {
        std::fs::write(&path, text).unwrap();
        let error = Registry::update(dir.path(), |r| {
            r.upsert_connection(Connection::new("c", "ably").unwrap())
        })
        .unwrap_err();
        assert_eq!(error.code(), "corrupt");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            text,
            "never overwritten"
        );
    }
}

#[cfg(unix)]
#[test]
fn hs2_8x2s08_the_registry_file_is_private_and_holds_no_credential() {
    use std::os::unix::fs::PermissionsExt;
    let dir = home();
    let secrets = Memory::default();
    let wf = Workflow {
        home: dir.path(),
        adapters: &[fake_adapter()],
        secrets: &secrets,
    };
    wf.add_connection("c1", "fake", BTreeMap::new()).unwrap();
    wf.set_secret("c1", Some("synthetic-credential-value"))
        .unwrap();
    let path = registry_path(dir.path());
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("synthetic-credential-value"), "{text}");
}

#[test]
fn hs2_8x2s08_concurrent_updates_never_lose_each_others_changes() {
    let dir = home();
    std::thread::scope(|scope| {
        for n in 0..8 {
            let path = dir.path();
            scope.spawn(move || {
                Registry::update(path, |r| {
                    r.upsert_connection(Connection::new(&format!("c{n}"), "ably").unwrap())
                })
                .unwrap();
            });
        }
    });
    assert_eq!(Registry::load(dir.path()).unwrap().connections.len(), 8);
}

// ---- the fake adapter and listener --------------------------------------------------

enum End {
    Block,
    Fail(AdapterError),
}

struct Session {
    events: Vec<Event>,
    end: End,
}

#[derive(Default)]
struct FakeAdapter {
    sessions: Mutex<VecDeque<Session>>,
    runs: AtomicUsize,
    channels: Mutex<Vec<Vec<String>>>,
    credentials: Mutex<Vec<Option<String>>>,
    test_result: Mutex<Option<AdapterError>>,
}

impl ChangeAdapter for FakeAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            id: "fake",
            display_name: "Fake",
            contract: CONTRACT_VERSION,
            settings: vec![SettingSpec {
                key: "endpoint",
                label: "Endpoint",
                required: false,
                help: "",
            }],
            credential: true,
            credential_help: "a synthetic key",
        }
    }

    fn run(
        &self,
        context: &ConnectionContext,
        channels: &[String],
        emit: &mut dyn FnMut(Event),
        stop: &AtomicBool,
    ) -> Result<(), AdapterError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        self.channels.lock().unwrap().push(channels.to_vec());
        self.credentials
            .lock()
            .unwrap()
            .push(context.credential.clone());
        let session = self.sessions.lock().unwrap().pop_front();
        let Some(session) = session else {
            emit(Event::Connected { resumed: true });
            return block(stop);
        };
        for event in session.events {
            emit(event);
        }
        match session.end {
            End::Block => block(stop),
            End::Fail(error) => Err(error),
        }
    }

    fn test(&self, _: &ConnectionContext) -> Result<(), AdapterError> {
        self.test_result.lock().unwrap().clone().map_or(Ok(()), Err)
    }
}

fn block(stop: &AtomicBool) -> Result<(), AdapterError> {
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

fn fake_adapter() -> Arc<dyn ChangeAdapter> {
    Arc::new(FakeAdapter::default())
}

#[derive(Default)]
struct Recorder {
    changes: Mutex<Vec<RemoteChange>>,
    rechecks: Mutex<Vec<String>>,
    health: Mutex<Vec<Health>>,
    /// Guards only the wake-up, so a waiting condition may lock the lists above.
    gate: Mutex<()>,
    signal: Condvar,
}

impl ChangeListener for Recorder {
    fn changed(&self, change: &RemoteChange) {
        self.changes.lock().unwrap().push(change.clone());
        self.notify();
    }
    fn recheck(&self, source: &str, _: RecheckReason) {
        self.rechecks.lock().unwrap().push(source.to_owned());
        self.notify();
    }
    fn health(&self, _: &str, health: &Health) {
        self.health.lock().unwrap().push(health.clone());
        self.notify();
    }
}

impl Recorder {
    fn notify(&self) {
        let _held = self.gate.lock().unwrap();
        self.signal.notify_all();
    }

    fn wait(&self, what: &str, condition: impl Fn(&Recorder) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut gate = self.gate.lock().unwrap();
        while !condition(self) {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "timed out waiting for {what}");
            // A short wake also covers a condition that watches something this recorder is
            // not told about (the adapter's run counter).
            gate = self
                .signal
                .wait_timeout(gate, left.min(Duration::from_millis(20)))
                .unwrap()
                .0;
        }
    }
}

struct Rig {
    dir: tempfile::TempDir,
    adapter: Arc<FakeAdapter>,
    secrets: Arc<Memory>,
    recorder: Arc<Recorder>,
    hub: Hub,
}

fn rig() -> Rig {
    let dir = home();
    let adapter = Arc::new(FakeAdapter::default());
    let secrets = Arc::new(Memory::default());
    let recorder = Arc::new(Recorder::default());
    let hub = Hub::start(
        dir.path().to_path_buf(),
        vec![adapter.clone() as Arc<dyn ChangeAdapter>],
        secrets.clone(),
        recorder.clone(),
        Backoff {
            start: Duration::from_millis(5),
            cap: Duration::from_millis(20),
        },
        Duration::ZERO,
        Arc::new(|| {
            u64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
            )
            .unwrap()
        }),
    );
    Rig {
        dir,
        adapter,
        secrets,
        recorder,
        hub,
    }
}

impl Rig {
    fn wf(&self) -> Workflow<'_> {
        // The workflow borrows the adapters; a throwaway slice of the same adapter suffices.
        Workflow {
            home: self.dir.path(),
            adapters: std::slice::from_ref(Box::leak(Box::new(
                self.adapter.clone() as Arc<dyn ChangeAdapter>
            ))),
            secrets: self.secrets.as_ref(),
        }
    }

    fn connect(&self, id: &str, bindings: &[(&str, &str, &str)]) {
        self.wf()
            .add_connection(id, "fake", BTreeMap::new())
            .unwrap();
        self.wf()
            .set_secret(id, Some("synthetic-credential"))
            .unwrap();
        for (source, channel, name) in bindings {
            self.wf()
                .bind(source, id, channel, repo(name), vec![MAIN.into()])
                .unwrap();
        }
        self.hub.reload().unwrap();
    }
}

fn connected(resumed: bool) -> Event {
    Event::Connected { resumed }
}

fn push(channel: &str, name: &str, tip: &str, event: &str) -> Event {
    Event::Hint {
        channel: channel.into(),
        hint: hint(name, MAIN, tip, event),
    }
}

// ---- the workflow -------------------------------------------------------------------

#[test]
fn hs2_8x2s08_the_workflow_is_idempotent_secret_free_and_names_the_next_step() {
    let dir = home();
    let secrets = Memory::default();
    let adapters = [fake_adapter()];
    let wf = Workflow {
        home: dir.path(),
        adapters: &adapters,
        secrets: &secrets,
    };

    let (change, next) = wf.add_connection("c1", "fake", BTreeMap::new()).unwrap();
    assert_eq!(change, Change::Created);
    assert!(next[0].contains("connection secret c1"), "{next:?}");
    assert_eq!(
        wf.add_connection("c1", "fake", BTreeMap::new()).unwrap().0,
        Change::Unchanged
    );

    assert_eq!(
        wf.list().unwrap()["connections"][0]["credential"],
        "missing"
    );
    assert_eq!(
        wf.set_secret("c1", Some("synthetic-credential\n")).unwrap(),
        Change::Created
    );
    assert_eq!(
        wf.set_secret("c1", Some("synthetic-credential")).unwrap(),
        Change::Unchanged
    );
    assert_eq!(
        wf.set_secret("c1", Some("another-synthetic-credential"))
            .unwrap(),
        Change::Updated
    );
    let listed = wf.list().unwrap().to_string();
    assert!(listed.contains("\"stored\""), "{listed}");
    assert!(
        !listed.contains("synthetic-credential"),
        "a credential is never listed: {listed}"
    );
    assert!(
        wf.add_connection("c1", "fake", BTreeMap::new())
            .unwrap()
            .1
            .is_empty(),
        "nothing left to do"
    );

    assert_eq!(
        wf.bind("store:t", "c1", "ch", repo("acme/tickets"), vec![])
            .unwrap(),
        Change::Created
    );
    assert_eq!(
        wf.show("c1").unwrap()["bindings"][0]["refs"][0],
        DEFAULT_REF
    );
    assert_eq!(wf.remove_connection("c1").unwrap_err().code(), "conflict");
    assert_eq!(wf.unbind("store:t", "c1", "ch").unwrap(), Change::Removed);
    assert_eq!(wf.remove_connection("c1").unwrap(), Change::Removed);
    assert!(
        secrets.get("remote-change.c1").unwrap().is_none(),
        "the credential goes with its connection"
    );
    assert_eq!(wf.set_secret("c1", None).unwrap_err().code(), "not_found");
}

#[test]
fn hs2_8x2s08_an_unknown_plugin_and_a_secret_shaped_setting_are_refused_explicitly() {
    let dir = home();
    let secrets = Memory::default();
    let adapters = [fake_adapter()];
    let wf = Workflow {
        home: dir.path(),
        adapters: &adapters,
        secrets: &secrets,
    };
    let unsupported = wf
        .add_connection("c1", "carrier-pigeon", BTreeMap::new())
        .unwrap_err();
    assert_eq!(unsupported.code(), "unsupported_plugin");
    assert!(
        unsupported.to_string().contains("available: fake"),
        "{unsupported}"
    );
    let mut settings = BTreeMap::new();
    settings.insert("api_key".to_owned(), "pasted-secret-value".to_owned());
    let refused = wf.add_connection("c1", "fake", settings).unwrap_err();
    assert!(
        refused.to_string().contains("has no setting 'api_key'"),
        "{refused}"
    );
    assert!(!refused.to_string().contains("pasted-secret-value"));
    assert!(
        Registry::load(dir.path()).unwrap().connections.is_empty(),
        "nothing was written"
    );
}

#[test]
fn hs2_8x2s08_a_test_reports_missing_credentials_and_adapter_failures_without_secrets() {
    let dir = home();
    let secrets = Memory::default();
    let adapter = Arc::new(FakeAdapter::default());
    let adapters = [adapter.clone() as Arc<dyn ChangeAdapter>];
    let wf = Workflow {
        home: dir.path(),
        adapters: &adapters,
        secrets: &secrets,
    };
    wf.add_connection("c1", "fake", BTreeMap::new()).unwrap();
    let missing = wf.test("c1").unwrap();
    assert!(!missing.ok);
    assert_eq!(missing.code, Some("missing_credential"));
    assert!(missing.next_steps[0].contains("connection secret c1"));
    wf.set_secret("c1", Some("synthetic-credential")).unwrap();
    assert!(wf.test("c1").unwrap().ok);
    *adapter.test_result.lock().unwrap() = Some(AdapterError::Auth("key refused".into()));
    let refused = wf.test("c1").unwrap();
    assert_eq!(refused.code, Some("auth_failed"));
    assert!(
        !serde_json::to_string(&refused)
            .unwrap()
            .contains("synthetic-credential")
    );
    assert_eq!(wf.test("ghost").unwrap_err().code(), "not_found");
}

#[test]
fn hs2_8x2s08_connection_contexts_never_print_their_credential() {
    let context = ConnectionContext {
        connection: "c1".into(),
        settings: BTreeMap::new(),
        credential: Some("synthetic-credential".into()),
    };
    let printed = format!("{context:?}");
    assert!(!printed.contains("synthetic-credential"), "{printed}");
    assert!(printed.contains("redacted"));
}

// ---- the hub ------------------------------------------------------------------------

#[test]
fn hs2_8x2s08_a_connection_serves_every_bound_source_with_one_subscription_and_rechecks_each() {
    let r = rig();
    r.adapter.sessions.lock().unwrap().push_back(Session {
        events: vec![
            connected(false),
            push("shared", "acme/app", TIP_A, "e1"),
            push("shared", "acme/app", TIP_A, "e1"),
            push("unbound", "acme/app", TIP_B, "e2"),
            push("shared", "evil/app", TIP_B, "e3"),
        ],
        end: End::Block,
    });
    r.connect(
        "c1",
        &[
            ("checkout:code", "shared", "acme/app"),
            ("store:tickets", "shared", "acme/app"),
        ],
    );
    r.recorder.wait("two deliveries", |rec| {
        rec.changes.lock().unwrap().len() >= 2
    });
    let mut sources: Vec<String> = r
        .recorder
        .changes
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.source.clone())
        .collect();
    sources.sort();
    assert_eq!(sources, ["checkout:code", "store:tickets"]);
    let mut rechecks = r.recorder.rechecks.lock().unwrap().clone();
    rechecks.sort();
    assert_eq!(
        rechecks,
        ["checkout:code", "store:tickets"],
        "every bound source is checked once the connection is up"
    );
    assert_eq!(r.adapter.runs.load(Ordering::SeqCst), 1);
    assert_eq!(
        r.adapter.channels.lock().unwrap()[0],
        ["shared"],
        "one subscription for the shared channel"
    );
    assert_eq!(
        r.adapter.credentials.lock().unwrap()[0].as_deref(),
        Some("synthetic-credential")
    );
    assert!(matches!(r.hub.health()["c1"], Health::Connected { .. }));
    let ignored = r.hub.ignored();
    assert_eq!(ignored["unknown_channel"], 1);
    assert_eq!(ignored["wrong_repository"], 1);
    assert_eq!(ignored["duplicate"], 2, "the repeat, once per source");
    assert_eq!(
        r.recorder.changes.lock().unwrap().len(),
        2,
        "nothing else was delivered"
    );
}

#[test]
fn hs2_8x2s08_a_dropped_connection_backs_off_reconnects_and_rechecks_only_after_a_gap() {
    let r = rig();
    {
        let mut sessions = r.adapter.sessions.lock().unwrap();
        sessions.push_back(Session {
            events: vec![connected(false)],
            end: End::Fail(AdapterError::Transient("reset".into())),
        });
        // Resumed over the gap: nothing was missed, so no recheck.
        sessions.push_back(Session {
            events: vec![connected(true)],
            end: End::Fail(AdapterError::Transient("reset again".into())),
        });
        // A reconnect that cannot vouch for the gap asks for a check.
        sessions.push_back(Session {
            events: vec![connected(false)],
            end: End::Block,
        });
    }
    r.connect("c1", &[("checkout:code", "ch", "acme/app")]);
    r.recorder.wait("the third connection", |rec| {
        rec.rechecks.lock().unwrap().len() >= 2
    });
    assert_eq!(r.adapter.runs.load(Ordering::SeqCst), 3);
    assert_eq!(
        r.recorder.rechecks.lock().unwrap().len(),
        2,
        "first connect and the unresumed reconnect, not the resumed one"
    );
    let health = r.recorder.health.lock().unwrap().clone();
    assert!(health.iter().any(|h| matches!(h, Health::Reconnecting { error, retry_in_ms } if error == "reset" && *retry_in_ms == 5)), "{health:?}");
    assert!(health.iter().any(|h| matches!(h, Health::Reconnecting { error, retry_in_ms } if error == "reset again" && *retry_in_ms == 5)), "{health:?}");
}

#[test]
fn hs2_8x2s08_the_reconnect_delay_doubles_to_a_cap() {
    let backoff = Backoff {
        start: Duration::from_millis(500),
        cap: Duration::from_secs(60),
    };
    let delays: Vec<u64> = (1..=9)
        .map(|n| u64::try_from(backoff.delay(n).as_millis()).unwrap())
        .collect();
    assert_eq!(
        delays,
        [500, 1000, 2000, 4000, 8000, 16000, 32000, 60000, 60000]
    );
    assert_eq!(backoff.delay(0), Duration::from_millis(500));
    assert_eq!(
        backoff.delay(u32::MAX),
        Duration::from_secs(60),
        "no overflow"
    );
}

#[test]
fn hs2_8x2s08_a_refused_credential_or_bad_configuration_stops_retrying() {
    for (error, expect) in [
        (AdapterError::Auth("401".into()), "auth_failed"),
        (AdapterError::Config("bad endpoint".into()), "misconfigured"),
    ] {
        let r = rig();
        r.adapter.sessions.lock().unwrap().push_back(Session {
            events: vec![],
            end: End::Fail(error),
        });
        r.connect("c1", &[("checkout:code", "ch", "acme/app")]);
        r.recorder.wait("a terminal health", |rec| {
            rec.health
                .lock()
                .unwrap()
                .iter()
                .any(|h| matches!(h, Health::AuthFailed { .. } | Health::Misconfigured { .. }))
        });
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            r.adapter.runs.load(Ordering::SeqCst),
            1,
            "{expect}: no retry loop"
        );
        let health = r.hub.health()["c1"].clone();
        match (&health, expect) {
            (Health::AuthFailed { .. }, "auth_failed")
            | (Health::Misconfigured { .. }, "misconfigured") => {}
            other => panic!("{other:?}"),
        }
        // Changing the connection restarts it with a fresh attempt.
        r.wf()
            .add_connection(
                "c1",
                "fake",
                [("endpoint".to_owned(), "https://example.invalid".to_owned())].into(),
            )
            .unwrap();
        r.hub.reload().unwrap();
        r.recorder.wait("a second attempt", |_| {
            r.adapter.runs.load(Ordering::SeqCst) >= 2
        });
    }
}

#[test]
fn hs2_8x2s08_a_missing_credential_or_plugin_is_misconfiguration_not_a_loop() {
    let r = rig();
    r.wf()
        .add_connection("c1", "fake", BTreeMap::new())
        .unwrap();
    r.wf()
        .bind("checkout:code", "c1", "ch", repo("acme/app"), vec![])
        .unwrap();
    r.hub.reload().unwrap();
    r.recorder.wait("misconfigured", |rec| {
        rec.health.lock().unwrap().iter().any(|h| matches!(h, Health::Misconfigured { error } if error.contains("connection secret c1")))
    });
    assert_eq!(
        r.adapter.runs.load(Ordering::SeqCst),
        0,
        "no credential, so the adapter never ran"
    );

    // A registry that names a plugin this build lacks says so and does not crash the hub.
    Registry::update(r.dir.path(), |reg| {
        reg.upsert_connection(Connection::new("c2", "ghost").unwrap())?;
        reg.bind(binding("store:t", "c2", "ch", "acme/tickets"))
    })
    .unwrap();
    r.hub.reload().unwrap();
    r.recorder.wait("unknown plugin", |rec| {
        rec.health.lock().unwrap().iter().any(|h| matches!(h, Health::Misconfigured { error } if error.contains("no adapter 'ghost'")))
    });
}

#[test]
fn hs2_8x2s08_reloading_restarts_only_what_changed_and_unbinding_stops_a_connection() {
    let r = rig();
    r.connect("c1", &[("checkout:code", "one", "acme/app")]);
    r.recorder
        .wait("first run", |_| r.adapter.runs.load(Ordering::SeqCst) == 1);

    // Nothing changed: the healthy connection is left alone.
    r.hub.reload().unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(r.adapter.runs.load(Ordering::SeqCst), 1);

    // A new channel changes the subscription set, so the connection restarts with both.
    r.wf()
        .bind(
            "store:tickets",
            "c1",
            "two",
            repo("acme/tickets"),
            vec![MAIN.into()],
        )
        .unwrap();
    r.hub.reload().unwrap();
    r.recorder
        .wait("restart", |_| r.adapter.runs.load(Ordering::SeqCst) == 2);
    assert_eq!(r.adapter.channels.lock().unwrap()[1], ["one", "two"]);

    // Unbinding everything stops the connection and reads idle.
    r.wf().unbind("checkout:code", "c1", "one").unwrap();
    r.wf().unbind("store:tickets", "c1", "two").unwrap();
    r.hub.reload().unwrap();
    assert_eq!(r.hub.health()["c1"], Health::Idle);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        r.adapter.runs.load(Ordering::SeqCst),
        2,
        "no new run for an unbound connection"
    );
}

#[test]
fn hs2_8x2s08_an_unconfigured_hub_does_nothing_and_shutdown_joins_every_thread() {
    let r = rig();
    r.hub.reload().unwrap();
    assert!(r.hub.health().is_empty());
    assert_eq!(
        r.adapter.runs.load(Ordering::SeqCst),
        0,
        "unbound projects keep their behavior"
    );
    r.connect("c1", &[("checkout:code", "ch", "acme/app")]);
    r.recorder
        .wait("connected", |_| r.adapter.runs.load(Ordering::SeqCst) == 1);
    r.hub.shutdown();
    r.hub.shutdown();
}

#[test]
fn hs2_8x2s08_a_burst_through_the_hub_is_coalesced_by_the_dispatcher_without_polling() {
    let dir = home();
    let adapter = Arc::new(FakeAdapter::default());
    let secrets = Arc::new(Memory::default());
    let recorder = Arc::new(Recorder::default());
    let hub = Hub::start(
        dir.path().to_path_buf(),
        vec![adapter.clone() as Arc<dyn ChangeAdapter>],
        secrets.clone(),
        recorder.clone(),
        Backoff {
            start: Duration::from_millis(5),
            cap: Duration::from_millis(20),
        },
        Duration::from_millis(300),
        Arc::new(|| {
            u64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
            )
            .unwrap()
        }),
    );
    let wf = Workflow {
        home: dir.path(),
        adapters: std::slice::from_ref(Box::leak(Box::new(
            adapter.clone() as Arc<dyn ChangeAdapter>
        ))),
        secrets: secrets.as_ref(),
    };
    wf.add_connection("c1", "fake", BTreeMap::new()).unwrap();
    wf.set_secret("c1", Some("synthetic-credential")).unwrap();
    wf.bind(
        "checkout:code",
        "c1",
        "ch",
        repo("acme/app"),
        vec![MAIN.into()],
    )
    .unwrap();
    adapter.sessions.lock().unwrap().push_back(Session {
        events: vec![
            connected(false),
            push("ch", "acme/app", TIP_A, "e1"),
            push("ch", "acme/app", TIP_B, "e2"),
            push(
                "ch",
                "acme/app",
                "cccccccccccccccccccccccccccccccccccccccc",
                "e3",
            ),
        ],
        end: End::Block,
    });
    hub.reload().unwrap();
    recorder.wait("leading and trailing", |rec| {
        rec.changes.lock().unwrap().len() >= 2
    });
    std::thread::sleep(Duration::from_millis(400));
    let changes = recorder.changes.lock().unwrap().clone();
    assert_eq!(
        changes.len(),
        2,
        "one leading and one trailing delivery for three hints: {changes:?}"
    );
    assert_eq!(changes[0].advertised_tip.as_deref(), Some(TIP_A));
    assert_eq!(
        changes[1].advertised_tip.as_deref(),
        Some("cccccccccccccccccccccccccccccccccccccccc")
    );
    hub.shutdown();
}
