//! HS2-WJF0D8: two clones of one bare store remote, bound to a `git_ref` configuration,
//! claim through the remote's compare-and-swap. Synthetic names only; every repository is
//! a temporary directory and nothing touches a credential store.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

use crate::claim_backends::{BackendConfiguration, BackendKind, Scope, git_store_sources};
use crate::ops::{self, NewTicket, OpError};
use crate::{FsStore, StoreError, StoreMetadata};

use super::*;

fn git(dir: &Path, args: &[&str]) -> String {
    let output = crate::git::command()
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

struct Fleet {
    _dir: tempfile::TempDir,
    bare: PathBuf,
    a: FsStore,
    b: FsStore,
    tickets: Vec<Ulid>,
}

fn now() -> Timestamp {
    Timestamp::from_datetime(time::OffsetDateTime::now_utc())
}

fn lease(minutes: i64) -> Timestamp {
    now().plus_minutes(minutes)
}

/// A bare store remote with `count` tickets, a project binding of the store identity to
/// a `git_ref` configuration with `offline`, and two clones.
fn fleet(count: usize, offline: OfflinePolicy) -> Fleet {
    fleet_with(count, offline, "shared", false)
}

/// [`fleet`] with an explicit configuration id, optionally naming a credential-store key
/// (HS2-EWGPTZ). A keyed configuration id is unique per test, because the key account is
/// derived from it and the machine home is shared by the test process.
fn fleet_with(count: usize, offline: OfflinePolicy, id: &str, credential: bool) -> Fleet {
    let dir = tempfile::tempdir().unwrap();
    let bare = dir.path().join("store.git");
    let (a_path, b_path) = (dir.path().join("a"), dir.path().join("b"));
    git(
        dir.path(),
        &[
            "init",
            "--quiet",
            "--bare",
            "-b",
            "main",
            bare.to_str().unwrap(),
        ],
    );
    git(
        dir.path(),
        &["init", "--quiet", "-b", "main", a_path.to_str().unwrap()],
    );
    for repo in [&bare, &a_path] {
        git(repo, &["config", "maintenance.auto", "false"]);
    }
    git(
        &a_path,
        &["remote", "add", "origin", bare.to_str().unwrap()],
    );
    let a = FsStore::init(&a_path, &StoreMetadata::new("CL"))
        .unwrap()
        .with_deferred_push();
    let mut tickets = Vec::new();
    for n in 0..count {
        let id = Ulid::new();
        ops::create(
            &a,
            id,
            "CL",
            now(),
            NewTicket {
                title: format!("synthetic ticket {n}"),
                ..NewTicket::default()
            },
        )
        .unwrap();
        tickets.push(id);
    }
    let source = git_store_sources(&a).remove(0);
    Scope::Project {
        root: a_path.clone(),
    }
    .update(|r| {
        let mut config = BackendConfiguration::new(id, BackendKind::GitRef)?;
        config.offline = offline;
        if credential {
            config.credential = Some(BackendConfiguration::default_credential(id));
        }
        r.upsert_configuration(config)?;
        r.bind_source(&source, id);
        Ok(())
    })
    .unwrap();
    git(&a_path, &["add", "-A"]);
    git(
        &a_path,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "commit",
            "-q",
            "-m",
            "seed",
        ],
    );
    git(&a_path, &["push", "-q", "-u", "origin", "main"]);
    git(
        dir.path(),
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            b_path.to_str().unwrap(),
        ],
    );
    git(&b_path, &["config", "maintenance.auto", "false"]);
    let b = FsStore::open(&b_path).unwrap().with_deferred_push();
    Fleet {
        _dir: dir,
        bare,
        a,
        b,
        tickets,
    }
}

fn marker(fleet: &Fleet, id: &Ulid) -> Option<RefMarker> {
    GitRefCas::new(fleet.a.root(), "origin")
        .read(&GitRefCas::ticket_ref(id))
        .unwrap()
        .map(|(_, marker)| marker)
}

fn claim(store: &FsStore, id: &Ulid, worker: &str) -> Result<Ticket, OpError> {
    ops::claim(store, id, &now(), lease(30), worker, None)
}

fn go_offline(store: &FsStore) {
    let gone = store.root().with_file_name("gone.git");
    git(
        store.root(),
        &["remote", "set-url", "origin", gone.to_str().unwrap()],
    );
}

fn go_online(fleet: &Fleet, store: &FsStore) {
    git(
        store.root(),
        &["remote", "set-url", "origin", fleet.bare.to_str().unwrap()],
    );
}

#[test]
fn hs2_wjf0d8_both_clones_resolve_the_shared_binding() {
    let f = fleet(1, OfflinePolicy::FailClosed);
    for store in [&f.a, &f.b] {
        let gate = ticket_gate(store).unwrap().expect("bound");
        assert_eq!(gate.configuration, "shared");
        assert_eq!(gate.offline, OfflinePolicy::FailClosed);
    }
}

#[test]
fn hs2_wjf0d8_concurrent_claims_from_two_clones_have_exactly_one_winner() {
    let f = fleet(4, OfflinePolicy::FailClosed);
    for id in &f.tickets {
        let barrier = Arc::new(Barrier::new(2));
        let results: Vec<(String, Result<Ticket, OpError>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = [(&f.a, "worker-a"), (&f.b, "worker-b")]
                .into_iter()
                .map(|(store, worker)| {
                    let barrier = Arc::clone(&barrier);
                    scope.spawn(move || {
                        barrier.wait();
                        (worker.to_owned(), claim(store, id, worker))
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let winners: Vec<&str> = results
            .iter()
            .filter(|(_, r)| r.is_ok())
            .map(|(w, _)| w.as_str())
            .collect();
        assert_eq!(winners.len(), 1, "exactly one clone wins: {results:?}");
        let winner = winners[0];
        let (loser, refused) = results.iter().find(|(w, _)| w != winner).unwrap();
        match refused {
            Err(OpError::WrongWorker { holder, .. }) => assert_eq!(holder, winner),
            other => panic!("{loser} should be refused naming {winner}: {other:?}"),
        }
        // The backend names the winner, and the loser's own ticket now does too.
        assert_eq!(marker(&f, id).unwrap().worker, winner);
        let loser_store = if loser == "worker-a" { &f.a } else { &f.b };
        assert_eq!(
            loser_store.read_ticket(id).unwrap().claimed_by.as_deref(),
            Some(winner)
        );
    }
}

#[test]
fn hs2_wjf0d8_renew_and_release_mirror_the_marker_and_terminal_states_release_it() {
    let f = fleet(2, OfflinePolicy::FailClosed);
    let (first, second) = (f.tickets[0], f.tickets[1]);
    let claimed = claim(&f.a, &first, "worker-a").unwrap();
    let attempt = attempt_id(&claimed, "worker-a").unwrap().to_string();
    let held = marker(&f, &first).unwrap();
    assert_eq!(
        (held.worker.as_str(), held.claim_id.as_str()),
        ("worker-a", attempt.as_str())
    );

    // An idempotent retry by the holder keeps the same marker identity.
    claim(&f.a, &first, "worker-a").unwrap();
    assert_eq!(marker(&f, &first).unwrap().claim_id, attempt);

    let later = lease(90);
    ops::renew(&f.a, &first, now(), later.clone(), "worker-a").unwrap();
    assert_eq!(marker(&f, &first).unwrap().expiry_ms(), millis(&later));

    ops::release(&f.a, &first, now(), "worker-a", false).unwrap();
    assert!(marker(&f, &first).is_none(), "release deletes the marker");
    claim(&f.b, &first, "worker-b").unwrap();

    // Completing a claimed ticket releases its marker too.
    claim(&f.a, &second, "worker-a").unwrap();
    assert!(marker(&f, &second).is_some());
    ops::close(
        &f.a,
        &second,
        now(),
        hotsheet_model::CloseReason::Completed,
        None,
    )
    .unwrap();
    assert!(
        marker(&f, &second).is_none(),
        "a terminal state releases the marker"
    );
}

#[test]
fn hs2_wjf0d8_offline_fail_closed_claims_nothing() {
    let f = fleet(1, OfflinePolicy::FailClosed);
    let id = f.tickets[0];
    go_offline(&f.a);
    match claim(&f.a, &id, "worker-a") {
        Err(OpError::Store(StoreError::ClaimBackendUnreachable(message))) => {
            assert!(message.contains("'shared' is unreachable"), "{message}");
        }
        other => panic!("expected claim_backend_unreachable: {other:?}"),
    }
    let ticket = f.a.read_ticket(&id).unwrap();
    assert_eq!(ticket.claimed_by, None);
    assert!(ticket.claim_history.is_empty());
    let next = ops::claim_next(&f.a, &now(), lease(30), "worker-a", None);
    assert!(
        matches!(next, Err(StoreError::ClaimBackendUnreachable(_))),
        "{next:?}"
    );
}

#[test]
fn hs2_wjf0d8_provisional_claims_reconcile_and_flag_the_loser_on_renew() {
    let f = fleet(1, OfflinePolicy::Provisional);
    let id = f.tickets[0];
    go_offline(&f.a);
    claim(&f.a, &id, "worker-a").unwrap();
    assert!(
        provisional_claims(&f.a).contains_key(&id),
        "recorded as provisional"
    );

    // Meanwhile the other machine takes the ticket through the backend.
    claim(&f.b, &id, "worker-b").unwrap();
    go_online(&f, &f.a);
    match ops::renew(&f.a, &id, now(), lease(60), "worker-a") {
        Err(OpError::WrongWorker { holder, .. }) => assert_eq!(holder, "worker-b"),
        other => panic!("the provisional claim lost: {other:?}"),
    }
    assert_eq!(
        f.a.read_ticket(&id).unwrap().claimed_by.as_deref(),
        Some("worker-b")
    );
    assert!(provisional_claims(&f.a).is_empty());
}

#[test]
fn hs2_wjf0d8_a_provisional_claim_nobody_contested_is_confirmed() {
    let f = fleet(1, OfflinePolicy::Provisional);
    let id = f.tickets[0];
    go_offline(&f.a);
    claim(&f.a, &id, "worker-a").unwrap();
    go_online(&f, &f.a);
    ops::renew(&f.a, &id, now(), lease(60), "worker-a").unwrap();
    assert!(provisional_claims(&f.a).is_empty());
    assert_eq!(marker(&f, &id).unwrap().worker, "worker-a");
    // Now exclusive: the other clone is refused.
    assert!(matches!(
        claim(&f.b, &id, "worker-b"),
        Err(OpError::WrongWorker { .. })
    ));
}

#[test]
fn hs2_wjf0d8_reconcile_reports_a_claim_lost_to_another_machine() {
    let f = fleet(1, OfflinePolicy::Provisional);
    let id = f.tickets[0];
    go_offline(&f.a);
    claim(&f.a, &id, "worker-a").unwrap();
    claim(&f.b, &id, "worker-b").unwrap();
    go_online(&f, &f.a);
    let conflicts = reconcile(&f.a, &now()).unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(
        (conflicts[0].worker.as_str(), conflicts[0].holder.as_str()),
        ("worker-a", "worker-b")
    );
    assert_eq!(take_conflicts(&f.a), conflicts);
    assert!(take_conflicts(&f.a).is_empty());
    assert_eq!(
        f.a.read_ticket(&id).unwrap().claimed_by.as_deref(),
        Some("worker-b")
    );
}

/// HS2-ZR8B4N: the wire standing of one ticket's claim through every transition: none,
/// provisional while offline, lost once another machine wins, none once the loser was told,
/// provisional again on a new offline claim, and none once that claim is confirmed or
/// released. An exclusive claim never reads as provisional.
#[test]
fn hs2_zr8b4n_claim_standing_follows_provisional_lost_and_confirmed_claims() {
    let f = fleet(2, OfflinePolicy::Provisional);
    let (id, other) = (f.tickets[0], f.tickets[1]);
    let standing = |store: &FsStore, id: &Ulid| {
        let ticket = store.read_ticket(id).unwrap();
        let live = ticket
            .claim_lease_expires_at
            .as_ref()
            .is_some_and(|at| at.chronological_cmp(&now()) == Some(std::cmp::Ordering::Greater));
        ClaimStandings::read(store).of(&id.to_string(), ticket.claimed_by.as_deref(), live)
    };
    assert_eq!(standing(&f.a, &id), None);
    // Online: an exclusive claim has no standing.
    claim(&f.a, &other, "worker-a").unwrap();
    assert_eq!(standing(&f.a, &other), None);

    go_offline(&f.a);
    claim(&f.a, &id, "worker-a").unwrap();
    assert_eq!(standing(&f.a, &id), Some(ClaimStanding::Provisional));
    assert_eq!(
        standing(&f.a, &other),
        None,
        "only the offline claim is provisional"
    );

    claim(&f.b, &id, "worker-b").unwrap();
    go_online(&f, &f.a);
    reconcile(&f.a, &now()).unwrap();
    match standing(&f.a, &id) {
        Some(ClaimStanding::Lost { worker, holder, .. }) => {
            assert_eq!((worker.as_str(), holder.as_str()), ("worker-a", "worker-b"));
        }
        other => panic!("expected a lost claim: {other:?}"),
    }
    // Reading again changes nothing: the loss stays until the session is told.
    assert!(matches!(
        standing(&f.a, &id),
        Some(ClaimStanding::Lost { .. })
    ));
    acknowledge_conflicts(&f.a, &[id]);
    assert_eq!(
        standing(&f.a, &id),
        None,
        "the holder's claim reads as theirs"
    );

    // A new offline claim on the other ticket, then confirmed: provisional, then none.
    ops::release(&f.a, &other, now(), "worker-a", false).unwrap();
    go_offline(&f.a);
    claim(&f.a, &other, "worker-a").unwrap();
    assert_eq!(standing(&f.a, &other), Some(ClaimStanding::Provisional));
    go_online(&f, &f.a);
    ops::renew(&f.a, &other, now(), lease(60), "worker-a").unwrap();
    assert_eq!(standing(&f.a, &other), None);

    // A provisional record whose claim is no longer the ticket's live claim is ignored.
    let records = ClaimStandings {
        provisional: BTreeMap::from([(
            other,
            ProvisionalClaim {
                claim_id: "x".into(),
                worker: "worker-gone".into(),
            },
        )]),
        lost: BTreeMap::new(),
        now: now(),
    };
    assert_eq!(records.of(&other.to_string(), Some("worker-a"), true), None);
    assert_eq!(
        records.of(&other.to_string(), Some("worker-gone"), false),
        None
    );
    assert_eq!(
        records.of(&other.to_string(), Some("worker-gone"), true),
        Some(ClaimStanding::Provisional)
    );
    assert_eq!(records.of("not-a-ulid", Some("worker-gone"), true), None);
    // The wire shape.
    assert_eq!(
        serde_json::to_value(ClaimStanding::Provisional).unwrap(),
        serde_json::json!({"kind": "provisional"})
    );
}

#[test]
fn hs2_wjf0d8_claim_next_skips_a_ticket_another_machine_holds() {
    let f = fleet(2, OfflinePolicy::FailClosed);
    let first = f.tickets[0];
    claim(&f.b, &first, "worker-b").unwrap();
    // Clone A has not synced, so its own queue still offers the first ticket.
    let next = ops::claim_next(&f.a, &now(), lease(30), "worker-a", None)
        .unwrap()
        .unwrap();
    assert_eq!(next.id, f.tickets[1]);
    assert_eq!(
        f.a.read_ticket(&first).unwrap().claimed_by.as_deref(),
        Some("worker-b")
    );
}

#[test]
fn hs2_wjf0d8_an_unbound_store_never_touches_the_remote() {
    let f = fleet(1, OfflinePolicy::FailClosed);
    std::fs::remove_file(
        f.a.root()
            .join(crate::claim_backends::PROJECT_REGISTRY_FILE),
    )
    .unwrap();
    assert!(ticket_gate(&f.a).unwrap().is_none());
    go_offline(&f.a);
    claim(&f.a, &f.tickets[0], "worker-a").unwrap();
    go_online(&f, &f.a);
    assert!(marker(&f, &f.tickets[0]).is_none());
}

#[test]
fn hs2_wjf0d8_a_sync_that_pulls_flags_a_claim_lost_to_another_machine() {
    let f = fleet(1, OfflinePolicy::Provisional);
    let id = f.tickets[0];
    go_offline(&f.a);
    claim(&f.a, &id, "worker-a").unwrap();
    // Another machine takes the ticket through the backend; then any change reaches
    // the remote, so this clone's next sync pulls.
    claim(&f.b, &id, "worker-b").unwrap();
    let other = f.bare.with_file_name("c");
    git(
        f.bare.parent().unwrap(),
        &[
            "clone",
            "-q",
            f.bare.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join("README.md"), "synthetic\n").unwrap();
    git(&other, &["add", "README.md"]);
    git(
        &other,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "commit",
            "-q",
            "-m",
            "other",
        ],
    );
    git(&other, &["push", "-q", "origin", "main"]);
    go_online(&f, &f.a);
    crate::flush_background_commits();
    let report = crate::sync_once(f.a.root());
    assert!(
        matches!(report, crate::SyncReport::Synced { pulled: true, .. }),
        "{report:?}"
    );
    let conflicts = take_conflicts(&f.a);
    assert_eq!(conflicts.len(), 1, "{conflicts:?}");
    assert_eq!(
        (conflicts[0].worker.as_str(), conflicts[0].holder.as_str()),
        ("worker-a", "worker-b")
    );
    assert_eq!(
        f.a.read_ticket(&id).unwrap().claimed_by.as_deref(),
        Some("worker-b")
    );
    assert!(matches!(
        ops::renew(&f.a, &id, now(), lease(60), "worker-a"),
        Err(OpError::WrongWorker { holder, .. }) if holder == "worker-b"
    ));
}

// ---- HS2-EWGPTZ: holder fields never reach the backend in plain ----

fn unique_id(prefix: &str) -> String {
    format!("{prefix}-{}", Ulid::new().to_string().to_lowercase())
}

/// An in-memory credential store for this test's thread: `ticket_gate_at` reads it
/// instead of the native adapter (HS2-SFVWN5 forbids native credentials in unit tests).
#[derive(Default)]
struct MemorySecrets(std::sync::Mutex<std::collections::BTreeMap<String, String>>);

impl crate::secrets::SecretStore for MemorySecrets {
    fn set(&self, account: &str, secret: &str) -> Result<(), crate::secrets::SecretError> {
        self.0
            .lock()
            .unwrap()
            .insert(account.to_owned(), secret.to_owned());
        Ok(())
    }
    fn get(&self, account: &str) -> Result<Option<String>, crate::secrets::SecretError> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }
    fn delete(&self, account: &str) -> Result<bool, crate::secrets::SecretError> {
        Ok(self.0.lock().unwrap().remove(account).is_some())
    }
}

thread_local! {
    static SECRETS: Arc<MemorySecrets> = Arc::new(MemorySecrets::default());
    static INJECTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The gate's credential store when a keyed test is running on this thread.
pub(super) fn injected_secrets() -> Option<Arc<dyn crate::secrets::SecretStore>> {
    INJECTED
        .with(std::cell::Cell::get)
        .then(|| SECRETS.with(|s| Arc::clone(s) as Arc<dyn crate::secrets::SecretStore>))
}

fn account(id: &str) -> String {
    BackendConfiguration::default_credential(id)
}

fn put_key(id: &str, byte: u8) {
    use crate::secrets::SecretStore;
    use base64::Engine;
    INJECTED.with(|flag| flag.set(true));
    SECRETS
        .with(|s| {
            s.set(
                &account(id),
                &base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([byte; 32]),
            )
        })
        .unwrap();
}

fn drop_key(id: &str) {
    use crate::secrets::SecretStore;
    SECRETS.with(|s| s.delete(&account(id))).unwrap();
}

/// Every byte a backend holds for `id`, as the remote would show it.
fn stored_marker_text(f: &Fleet, id: &Ulid) -> String {
    let refname = GitRefCas::ticket_ref(id);
    let tree = git(&f.bare, &["ls-tree", "-r", &refname]);
    let mut text = tree.clone();
    for line in tree.lines() {
        if let Some(blob) = line.split_whitespace().nth(2) {
            text.push_str(&git(&f.bare, &["cat-file", "-p", blob]));
        }
    }
    text.push_str(&git(&f.bare, &["cat-file", "-p", &refname]));
    text
}

#[test]
fn hs2_ewgptz_no_plain_worker_id_reaches_the_backend_write() {
    let id = unique_id("keyed");
    put_key(&id, 7);
    let f = fleet_with(1, OfflinePolicy::FailClosed, &id, true);
    let ticket = f.tickets[0];
    ops::claim(
        &f.a,
        &ticket,
        &now(),
        lease(30),
        "worker-a-secret",
        Some("Build agent".into()),
    )
    .unwrap();
    let held = marker(&f, &ticket).unwrap();
    assert!(held.worker.starts_with("hmac-sha256:"), "{}", held.worker);
    assert!(held.metadata["label"].starts_with("enc:v1:"));
    let stored = stored_marker_text(&f, &ticket);
    assert!(stored.contains("hmac-sha256:"), "{stored}");
    assert!(
        !stored.contains("worker-a-secret"),
        "plain worker id leaked: {stored}"
    );
    assert!(
        !stored.contains("Build agent"),
        "plain label leaked: {stored}"
    );
    drop_key(&id);
}

#[test]
fn hs2_ewgptz_the_holder_renews_and_releases_its_own_keyed_claim() {
    let id = unique_id("keyed");
    put_key(&id, 7);
    let f = fleet_with(1, OfflinePolicy::FailClosed, &id, true);
    let ticket = f.tickets[0];
    claim(&f.a, &ticket, "worker-a").unwrap();
    // A retry and a renewal by the holder keep the same marker (ownership compares by digest).
    let attempt = marker(&f, &ticket).unwrap().claim_id;
    claim(&f.a, &ticket, "worker-a").unwrap();
    ops::renew(&f.a, &ticket, now(), lease(90), "worker-a").unwrap();
    assert_eq!(marker(&f, &ticket).unwrap().claim_id, attempt);
    ops::release(&f.a, &ticket, now(), "worker-a", false).unwrap();
    assert!(marker(&f, &ticket).is_none());
    drop_key(&id);
}

#[test]
fn hs2_ewgptz_the_loser_reads_the_winners_decrypted_label_and_hashing_still_compares() {
    let id = unique_id("keyed");
    put_key(&id, 7);
    let f = fleet_with(1, OfflinePolicy::FailClosed, &id, true);
    let ticket = f.tickets[0];
    ops::claim(
        &f.a,
        &ticket,
        &now(),
        lease(30),
        "worker-a",
        Some("Build agent".into()),
    )
    .unwrap();
    match claim(&f.b, &ticket, "worker-b") {
        Err(OpError::WrongWorker { holder, .. }) => assert_eq!(holder, "Build agent"),
        other => panic!("refused naming the label: {other:?}"),
    }
    let adopted = f.b.read_ticket(&ticket).unwrap();
    assert_eq!(adopted.worker_label.as_deref(), Some("Build agent"));
    let gate = ticket_gate(&f.b).unwrap().unwrap();
    let held = marker(&f, &ticket).unwrap();
    assert!(gate.owns(&held.worker, "worker-a"));
    assert!(!gate.owns(&held.worker, "worker-b"));
    drop_key(&id);
}

#[test]
fn hs2_ewgptz_a_machine_without_the_key_reads_the_holder_as_unreadable_and_writes_nothing() {
    let id = unique_id("keyed");
    put_key(&id, 7);
    let f = fleet_with(1, OfflinePolicy::FailClosed, &id, true);
    let ticket = f.tickets[0];
    ops::claim(
        &f.a,
        &ticket,
        &now(),
        lease(30),
        "worker-a",
        Some("Build agent".into()),
    )
    .unwrap();
    drop_key(&id);
    let gate = ticket_gate(&f.b).unwrap().unwrap();
    assert!(matches!(gate.holder, HolderKey::Missing { .. }));
    assert_eq!(
        gate.holder_name(&marker(&f, &ticket).unwrap()),
        UNREADABLE_HOLDER
    );
    // Claiming is refused before anything reaches the backend, naming the next step.
    let before = marker(&f, &ticket).unwrap();
    match claim(&f.b, &ticket, "worker-b") {
        Err(OpError::Store(StoreError::Io(error))) => {
            assert!(error.to_string().contains("--import"), "{error}");
        }
        other => panic!("a keyless claim must be refused: {other:?}"),
    }
    assert_eq!(
        marker(&f, &ticket).unwrap(),
        before,
        "the backend's marker is untouched"
    );
    assert_eq!(
        f.b.read_ticket(&ticket).unwrap().claimed_by,
        None,
        "nothing was written locally either"
    );
}

#[test]
fn hs2_ewgptz_importing_the_key_on_another_machine_restores_names_and_ownership() {
    let id = unique_id("keyed");
    put_key(&id, 7);
    let f = fleet_with(1, OfflinePolicy::FailClosed, &id, true);
    let ticket = f.tickets[0];
    ops::claim(
        &f.a,
        &ticket,
        &now(),
        lease(30),
        "worker-a",
        Some("Build agent".into()),
    )
    .unwrap();
    drop_key(&id);
    assert_eq!(
        ticket_gate(&f.b)
            .unwrap()
            .unwrap()
            .holder_name(&marker(&f, &ticket).unwrap()),
        UNREADABLE_HOLDER
    );
    put_key(&id, 7);
    let gate = ticket_gate(&f.b).unwrap().unwrap();
    assert_eq!(
        gate.holder_name(&marker(&f, &ticket).unwrap()),
        "Build agent"
    );
    drop_key(&id);
}

#[test]
fn hs2_ewgptz_a_rotated_key_makes_old_holders_unreadable_and_foreign_until_they_lapse() {
    let id = unique_id("keyed");
    put_key(&id, 7);
    let f = fleet_with(1, OfflinePolicy::FailClosed, &id, true);
    let ticket = f.tickets[0];
    ops::claim(
        &f.a,
        &ticket,
        &now(),
        lease(30),
        "worker-a",
        Some("Build agent".into()),
    )
    .unwrap();
    put_key(&id, 8);
    let gate = ticket_gate(&f.a).unwrap().unwrap();
    let held = marker(&f, &ticket).unwrap();
    assert_eq!(
        gate.holder_name(&held),
        UNREADABLE_HOLDER,
        "sealed under the old key"
    );
    assert!(
        !gate.owns(&held.worker, "worker-a"),
        "the old digest no longer compares"
    );
    // The old claim keeps excluding other claimants until its lease lapses.
    assert!(matches!(
        claim(&f.b, &ticket, "worker-b"),
        Err(OpError::WrongWorker { .. })
    ));
    drop_key(&id);
}

#[test]
fn hs2_ewgptz_an_unkeyed_configuration_keeps_plain_holder_fields() {
    let f = fleet(1, OfflinePolicy::FailClosed);
    let ticket = f.tickets[0];
    claim(&f.a, &ticket, "worker-a").unwrap();
    let held = marker(&f, &ticket).unwrap();
    assert_eq!(held.worker, "worker-a");
    assert!(held.metadata.is_empty());
}

#[test]
fn hs2_322zsz_a_store_bound_to_an_external_backend_refuses_to_claim_without_it() {
    let f = fleet(1, OfflinePolicy::FailClosed);
    let source = git_store_sources(&f.a).remove(0);
    Scope::Project {
        root: f.a.root().to_path_buf(),
    }
    .update(|r| {
        let mut config = BackendConfiguration::new("plug", BackendKind::External)?;
        config.settings.insert("plugin".into(), "vault".into());
        r.upsert_configuration(config)?;
        r.bind_source(&source, "plug");
        Ok(())
    })
    .unwrap();
    // Ticket claims do not route through external backends yet (HS2-3SJ8JX): refusing is
    // the only safe answer, because claiming locally would ignore the binding.
    match claim(&f.a, &f.tickets[0], "worker-a") {
        Err(OpError::Store(StoreError::Io(error))) => {
            assert!(error.to_string().contains("not supported yet"), "{error}");
        }
        other => panic!("expected a refusal: {other:?}"),
    }
    assert_eq!(f.a.read_ticket(&f.tickets[0]).unwrap().claimed_by, None);
}

// ---- HS2-68JAY3: the claim records follow the store's Git directory as it changes ----

fn lost_conflict(ticket: Ulid, worker: &str) -> ClaimConflict {
    ClaimConflict {
        ticket,
        slug: "CL-1".into(),
        worker: worker.into(),
        holder: "other-machine".into(),
        detected_at: now(),
    }
}

fn standing(store: &FsStore, ticket: Ulid) -> Option<ClaimStanding> {
    ClaimStandings::read(store).of(&ticket.to_string(), None, false)
}

fn canonical(path: &Path) -> PathBuf {
    hotsheet_sync::paths::canonicalize(path).unwrap()
}

/// A store first read while it is not a Git repository reads as having no records, as
/// often as it is asked; once it gains a `.git` the same process records and reads them
/// at once, with no restart.
#[test]
fn hs2_68jay3_a_store_that_gains_a_git_directory_gets_its_claim_records_without_restart() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path().join("store"), &StoreMetadata::new("CL")).unwrap();
    let ticket = Ulid::new();
    for _ in 0..3 {
        assert!(ClaimStandings::read(&store).is_empty());
        assert!(pending_conflicts(&store).is_empty());
        assert!(provisional_claims(&store).is_empty());
    }
    // Without a Git directory there is nowhere to record, and nothing is written.
    record_conflict(&store, lost_conflict(ticket, "worker-a"));
    assert!(pending_conflicts(&store).is_empty());
    assert!(record_provisional(&store, ticket, &Ulid::new(), "worker-a").is_err());

    git(store.root(), &["init", "--quiet", "-b", "main"]);
    let git_dir = canonical(&store.root().join(".git"));
    assert_eq!(
        git_path(&store, CONFLICTS_FILE).map(|path| canonical(path.parent().unwrap())),
        Some(git_dir.clone())
    );
    record_conflict(&store, lost_conflict(ticket, "worker-a"));
    assert_eq!(pending_conflicts(&store).len(), 1);
    assert!(matches!(
        standing(&store, ticket),
        Some(ClaimStanding::Lost { ref worker, .. }) if worker == "worker-a"
    ));
    let other = Ulid::new();
    record_provisional(&store, other, &Ulid::new(), "worker-b").unwrap();
    assert_eq!(provisional_claims(&store)[&other].worker, "worker-b");
    assert!(git_dir.join(PROVISIONAL_FILE).is_file());
}

/// Every transition of the store's Git directory is seen by the next lookup, whichever
/// of reads and writes comes first: removed, re-created, and acknowledged in between.
#[test]
fn hs2_68jay3_claim_records_follow_removed_and_recreated_git_directories() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path().join("store"), &StoreMetadata::new("CL")).unwrap();
    let (first, second) = (Ulid::new(), Ulid::new());
    git(store.root(), &["init", "--quiet", "-b", "main"]);
    // A write before any read, then the read.
    record_conflict(&store, lost_conflict(first, "worker-a"));
    assert!(standing(&store, first).is_some());

    // The repository goes away: nothing stale is reported, and writes go nowhere.
    std::fs::remove_dir_all(store.root().join(".git")).unwrap();
    assert!(ClaimStandings::read(&store).is_empty());
    assert!(pending_conflicts(&store).is_empty());
    record_conflict(&store, lost_conflict(second, "worker-b"));
    assert!(take_conflicts(&store).is_empty());

    // A new repository starts empty, then records again; a read comes before the write.
    git(store.root(), &["init", "--quiet", "-b", "main"]);
    assert!(ClaimStandings::read(&store).is_empty());
    record_conflict(&store, lost_conflict(second, "worker-b"));
    assert!(standing(&store, first).is_none());
    assert!(standing(&store, second).is_some());

    // Acknowledge (twice: the second is a no-op), refill and take.
    acknowledge_conflicts(&store, &[second]);
    assert!(pending_conflicts(&store).is_empty());
    acknowledge_conflicts(&store, &[second]);
    record_conflict(&store, lost_conflict(first, "worker-a"));
    assert_eq!(take_conflicts(&store).len(), 1);
    assert!(take_conflicts(&store).is_empty());
    assert!(ClaimStandings::read(&store).is_empty());
}

/// In a linked worktree the records live where `git rev-parse --git-path` puts them: the
/// worktree's own Git directory, not the shared one.
#[test]
fn hs2_68jay3_a_linked_worktree_keeps_its_records_in_its_own_git_directory() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "--quiet", "-b", "main"]);
    git(
        &main,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "seed",
        ],
    );
    let linked = dir.path().join("linked");
    git(
        &main,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "side",
            linked.to_str().unwrap(),
        ],
    );
    let store = FsStore::init(&linked, &StoreMetadata::new("CL")).unwrap();
    for name in [PROVISIONAL_FILE, CONFLICTS_FILE] {
        let expected = PathBuf::from(git(&linked, &["rev-parse", "--git-path", name]).trim());
        let expected = if expected.is_absolute() {
            expected
        } else {
            linked.join(expected)
        };
        let found = git_path(&store, name).unwrap();
        assert_eq!(found.file_name(), expected.file_name());
        assert_eq!(
            canonical(found.parent().unwrap()),
            canonical(expected.parent().unwrap())
        );
        assert_ne!(
            canonical(found.parent().unwrap()),
            canonical(&main.join(".git"))
        );
    }
    let ticket = Ulid::new();
    record_conflict(&store, lost_conflict(ticket, "worker-a"));
    assert!(standing(&store, ticket).is_some());
}

/// A record that cannot be saved or removed is reported and never panics; removing one
/// already gone is silent.
#[test]
fn hs2_68jay3_record_writes_that_fail_are_reported_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-dir").join(CONFLICTS_FILE);
    save_record(&missing, &Vec::<ClaimConflict>::new());
    assert!(!missing.exists());
    remove_record(&missing);
    let present = dir.path().join(CONFLICTS_FILE);
    save_record(&present, &vec![lost_conflict(Ulid::new(), "worker-a")]);
    assert_eq!(read_json::<Vec<ClaimConflict>>(&present).len(), 1);
    remove_record(&present);
    assert!(!present.exists());
    remove_record(&present);
}
