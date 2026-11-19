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
        let mut config = BackendConfiguration::new("shared", BackendKind::GitRef)?;
        config.offline = offline;
        r.upsert_configuration(config)?;
        r.bind_source(&source, "shared");
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
