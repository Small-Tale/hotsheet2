//! HS2-WDQZ37: a remote target's CI gate is watched on the machine that hosts the
//! target and followed over the reviewed transport with cursor replay. The hosted half
//! is a second runtime behind the real request dispatcher; its watch Job is judged in
//! its own Job store the way its monitor would. Synthetic data only.
use std::time::{Duration, Instant};

use hotsheet_jobs::JobStore;
use hotsheet_ticketing::orchestration::{GateError, GateState, TargetAction};
use hotsheet_ticketing::{NewTicket, ops};
use serde_json::json;

use super::gates::{judge, sha};
use super::tickets::{NOW, RemotePeer, machine, unsupported, worker};
use super::*;

pub(super) struct Remote {
    pub(super) peer: Arc<RemotePeer>,
    pub(super) runtime: OrchestrationRuntime,
    pub(super) owner: String,
    pub(super) run: String,
    _root: tempfile::TempDir,
}

impl Remote {
    pub(super) fn calls(&self, needle: &str) -> Vec<(String, String)> {
        self.peer
            .calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|(_, path)| path.contains(needle))
            .cloned()
            .collect()
    }
    fn answer_with(&self, status: Option<u16>) {
        *self
            .peer
            .status_override
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = status;
    }
    fn host_jobs(&self) -> Vec<hotsheet_jobs::Job> {
        JobStore::at(&self.peer.hosted.home)
            .list(&self.peer.hosted.target.id)
            .unwrap()
            .jobs
    }
    fn own_jobs(&self) -> usize {
        // The orchestrating machine's Job store, under the checkout the gate belongs to.
        JobStore::at(&self.runtime.home)
            .list(&self.owner)
            .unwrap()
            .jobs
            .len()
    }
}

/// An orchestrating machine with a run on a target another machine hosts.
pub(super) fn remote(tx: &mut Transaction<'_>) -> Remote {
    let peer = Arc::new(RemotePeer {
        hosted: machine(99, false, unsupported),
        calls: Mutex::new(Vec::new()),
        status_override: Mutex::new(None),
    });
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let project = root.path().join("owner");
    std::fs::create_dir(&project).unwrap();
    let store_path = root.path().join("owner-store");
    let owner_store = hotsheet_ticketing::FsStore::init_git(
        &store_path,
        &hotsheet_ticketing::StoreMetadata::new("OWN"),
    )
    .unwrap();
    let checkouts = CheckoutRegistry::new(home.join("checkouts.json"));
    let owner = checkouts
        .register(&project, None, None, vec![store_path])
        .unwrap();
    let secrets = Arc::new(Memory::default());
    let runtime = OrchestrationRuntime::new(
        home,
        checkouts,
        ProviderConfigRegistry::new(root.path().join("providers.json")),
        secrets.clone(),
        peer.clone(),
        id(10),
    )
    .unwrap();
    let target = TargetRef {
        owner_checkout_id: owner.id.clone(),
        target_id: id(1),
    };
    let manifest = runtime
        .replace(
            &owner.id,
            ReplaceTargets {
                expected_revision: None,
                targets: vec![TargetDeclaration {
                    target_id: id(1),
                    state: DeclarationState::Active,
                    label: "Remote".into(),
                    expected_repository_identity: Some(
                        "https://github.com/example/widgets.git".into(),
                    ),
                }],
            },
            tx,
        )
        .unwrap();
    let registered = runtime
        .remotes
        .register(
            RegisterRemote {
                id: id(20),
                label: "Synthetic peer".into(),
                origin: "http://127.0.0.1:8765".into(),
                expected_server_id: id(99),
                credentials: credentials(),
            },
            secrets.as_ref(),
            peer.as_ref(),
            100,
        )
        .unwrap();
    let projection = peer
        .hosted
        .runtime
        .checkout_projection(&peer.hosted.target.id)
        .unwrap();
    runtime
        .bind(
            BindTarget {
                target: target.clone(),
                expected_manifest_revision: manifest.revision.unwrap(),
                expected_binding_revision: 0,
                previous_destination: None,
                destination: TargetDestination::Remote {
                    registration_id: id(20),
                    identity: registered.remote.identity,
                    checkout_id: projection.checkout_id,
                    checkout_authority: projection.checkout_authority,
                },
            },
            tx,
        )
        .unwrap();
    allow_on(
        &runtime,
        &target.owner_checkout_id,
        &target.target_id,
        &[TargetAction::WorkStart],
        &[],
        tx,
    );
    let orchestration = ops::create(
        &owner_store,
        ulid::Ulid::new(),
        "OWN",
        NOW.into(),
        NewTicket {
            title: "Coordinate remotely".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let work = ops::create(
        &peer.hosted.target_store,
        ulid::Ulid::new(),
        "TGT",
        NOW.into(),
        NewTicket {
            title: "Remote work".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let run = runtime
        .run_dispatch(
            &owner.id,
            serde_json::from_value(json!({
                "ticket": {"native_id": orchestration.slug},
                "target_id": id(1), "connection_id": peer.hosted.target_connection(),
                "native_id": work.id.to_string(), "operation_id": "op-1",
                "worker_id": "remote-worker",
            }))
            .unwrap(),
            Some(&worker()),
            tx,
        )
        .unwrap()
        .run
        .run_id;
    Remote {
        peer,
        runtime,
        owner: owner.id,
        run,
        _root: root,
    }
}

/// Give a hosted checkout a Git remote.
pub(super) fn git_remote(root: &Path, url: &str) {
    for args in [&["init", "-q"][..], &["remote", "add", "origin", url][..]] {
        let status = hotsheet_ticketing::git::command_in(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }
}

fn link(r: &Remote, commit: &str, tx: &mut Transaction<'_>) -> GateView {
    r.runtime
        .gate_link(
            &r.owner,
            &r.run,
            serde_json::from_value(json!({"commit": commit})).unwrap(),
            Some(&worker()),
            tx,
        )
        .unwrap()
}

pub(super) fn sync(r: &Remote, wait_ms: u64, tx: &mut Transaction<'_>) -> RunGates {
    r.runtime
        .gate_sync(
            &r.owner,
            &r.run,
            &SyncGates { wait_ms },
            Some(&worker()),
            tx,
        )
        .unwrap()
}

#[test]
fn hs2_wdqz37_a_remote_targets_ci_gate_is_watched_on_its_host_and_followed_by_cursor() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let r = remote(&mut tx);
    let (a, b) = (sha('a'), sha('b'));
    let host_checkout = r.peer.hosted.target.id.clone();

    // HS2-K2YE6A: the host watches only a repository its own checkout names. Before
    // the hosted checkout has that remote, the link is refused by the gate's own code,
    // the host records the refusal and starts nothing.
    let refused = r.runtime.gate_link(
        &r.owner,
        &r.run,
        serde_json::from_value(json!({"commit": a})).unwrap(),
        Some(&worker()),
        &mut tx,
    );
    assert!(matches!(
        refused,
        Err(OrchestrationError::Gate(GateError::RepositoryMismatch))
    ));
    assert!(r.host_jobs().is_empty());
    let audit =
        hotsheet_ticketing::orchestration::AuditJournal::new(&r.peer.hosted.home, &host_checkout)
            .unwrap()
            .read(0, 50)
            .unwrap();
    assert!(audit.entries.iter().any(|entry| {
        entry.record.action == "hosted_gate_watch"
            && entry.record.outcome == hotsheet_ticketing::orchestration::AuditOutcome::Denied
            && entry.record.reason.as_deref() == Some("gate_repository_mismatch")
    }));
    git_remote(
        Path::new(&r.peer.hosted.target.root),
        "git@github.com:Example/widgets.git",
    );

    // Linking starts the watch on the hosting machine, with its credentials: the Job
    // lives in the host's store and the orchestrating machine keeps none.
    let linked = link(&r, &a, &mut tx);
    assert_eq!(linked.gate.state, GateState::Pending);
    let starts = r.calls("/target-gates");
    assert_eq!(starts.len(), 2, "the refused start, then this one");
    assert!(starts.iter().all(|(method, _)| method == "POST"));
    let jobs = r.host_jobs();
    assert_eq!(jobs.len(), 1);
    assert_eq!(linked.gate.watch_ref.as_deref(), Some(jobs[0].id.as_str()));
    assert!(
        jobs[0].ticket.is_none(),
        "nothing on the host is tied to a ticket"
    );
    assert_eq!(r.own_jobs(), 0);
    // Linking again starts nothing new there.
    link(&r, &a, &mut tx);
    assert_eq!(r.host_jobs().len(), 1);

    // The host's monitor judges the run green while a sync waits on the host's long
    // poll: the wait ends on that change, not on a timer.
    let (home, job) = (r.peer.hosted.home.clone(), jobs[0].id.clone());
    let (checkout, commit) = (host_checkout.clone(), a.clone());
    let judged = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        judge(&home, &checkout, &job, "green", &commit, 61);
    });
    let started = Instant::now();
    let mut green = sync(&r, 15_000, &mut tx);
    while green.summary.ci == GateState::Pending {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the verdict never ended a wait"
        );
        green = sync(&r, 15_000, &mut tx);
    }
    judged.join().unwrap();
    assert_eq!(green.summary.ci, GateState::Passed);
    let evidence = green.summary.gates[0].evidence.clone().unwrap();
    assert_eq!(
        (evidence.reference.as_str(), evidence.commit.as_str()),
        ("61", a.as_str())
    );
    let waits: Vec<String> = r
        .calls("/target-gates?")
        .into_iter()
        .map(|(_, path)| path)
        .filter(|path| path.contains("&after="))
        .collect();
    assert!(
        waits.iter().any(|path| path.contains("wait_ms=8000")),
        "the wait is the host's long poll: {waits:?}"
    );
    assert_eq!(r.own_jobs(), 0);

    // Cursor replay: an older cursor is answered at once; the current one waits.
    let watch = HostedGateWatch {
        commit: a.clone(),
        repository: "example/widgets".into(),
        workflow: "CI".into(),
        branch: "main".into(),
        artifact: None,
        again: false,
    };
    let hosted = &r.peer.hosted.runtime;
    let asked = Instant::now();
    let replayed = hosted
        .hosted_gate(&host_checkout, &watch, Some(0), MAX_HOSTED_GATE_WAIT_MS)
        .unwrap();
    assert!(asked.elapsed() < Duration::from_secs(2));
    assert_eq!(replayed.state, GateState::Passed);
    let asked = Instant::now();
    let unchanged = hosted
        .hosted_gate(&host_checkout, &watch, Some(replayed.cursor), 300)
        .unwrap();
    assert!(asked.elapsed() >= Duration::from_millis(250));
    assert_eq!(unchanged.cursor, replayed.cursor);
    assert!(matches!(
        hosted.hosted_gate(&host_checkout, &watch, None, MAX_HOSTED_GATE_WAIT_MS + 1),
        Err(OrchestrationError::Invalid(_))
    ));

    // An older peer without the route: this machine's adapter watches, as before.
    r.answer_with(Some(404));
    let fallback = link(&r, &b, &mut tx);
    assert_eq!(fallback.gate.state, GateState::Pending);
    assert_eq!(r.own_jobs(), 1);
    // The peer is upgraded: the next sync asks it, finds it does not watch the gate
    // yet, and has it watch there.
    r.answer_with(None);
    let healed = sync(&r, 0, &mut tx);
    assert_eq!(healed.summary.ci, GateState::Pending);
    let hosted_b = r
        .host_jobs()
        .into_iter()
        .find(|job| job.id != jobs[0].id)
        .expect("the host watches the commit now");
    assert_eq!(
        r.runtime
            .gate(&r.owner, &fallback.gate.gate_id)
            .unwrap()
            .watch_ref
            .as_deref(),
        Some(hosted_b.id.as_str())
    );

    // A peer that fails leaves the gate as it stands, and a sync does not fail.
    r.answer_with(Some(500));
    let failing = sync(&r, 0, &mut tx);
    assert_eq!(failing.summary.ci, GateState::Pending);
    assert!(failing.changed.is_empty());
    // The follower backs off instead of asking again at once.
    let mut follow = GateFollowState::default();
    let before = r.calls("/target-gates").len();
    let asked = Instant::now();
    let step = r
        .runtime
        .gate_follow_step(&r.owner, &mut follow, 600, &mut tx);
    assert!(step.active && step.changed.is_empty());
    assert!(asked.elapsed() >= Duration::from_millis(400));
    let asked_host = r.calls("/target-gates").len() - before;
    assert!(
        (1..=2).contains(&asked_host),
        "the host was asked, and not again at once: {asked_host}"
    );
    r.answer_with(None);

    // The follower long-polls the host and records its red verdict.
    let mut follow = GateFollowState::default();
    let (home, job, checkout, commit) = (
        r.peer.hosted.home.clone(),
        hosted_b.id.clone(),
        host_checkout.clone(),
        b.clone(),
    );
    let judged = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        judge(&home, &checkout, &job, "red", &commit, 62);
    });
    let started = Instant::now();
    loop {
        let step = r
            .runtime
            .gate_follow_step(&r.owner, &mut follow, 15_000, &mut tx);
        if !step.changed.is_empty() {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the follower never saw the verdict"
        );
    }
    judged.join().unwrap();
    let red = r.runtime.gate(&r.owner, &fallback.gate.gate_id).unwrap();
    assert_eq!(red.state, GateState::Failed);
    assert_eq!(red.evidence.unwrap().reference, "62");
    // Nothing is left to follow.
    assert!(
        !r.runtime
            .gate_follow_step(&r.owner, &mut follow, 0, &mut tx)
            .active
    );
}
