//! HS2-J8PRTV: target capacity and admission through the shared runtime, for a local
//! destination and a remote one. Scripted probes and synthetic identities only.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AdmissionDecision, AdmissionOutcome, AdmissionReason, AdmissionStore, AuditOutcome,
    CapacityField, CapacitySnapshot, CpuCapacity, DiskCapacity, HostObservation, MachineLimits,
    MemoryCapacity, PolicyBody, SessionFootprint, SetPolicy, TargetAction, TargetOpError,
    TargetPolicy, ToolAvailability, ToolPolicy, ToolScope,
};
use serde_json::{Value, json};

use super::tickets::{
    Machine, RemotePeer, bind_and_allow, human, machine, op, unsupported, worker,
};
use super::*;

/// A probe whose answer the test sets, counting how often the runtime asked.
#[derive(Default)]
struct Scripted {
    observation: Mutex<HostObservation>,
    asked: AtomicUsize,
}
impl Scripted {
    fn set(&self, change: impl FnOnce(&mut HostObservation)) {
        change(
            &mut self
                .observation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }
}
impl CapacityProbe for Scripted {
    fn observe(&self, _: &Checkout) -> HostObservation {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.observation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// An idle machine with room: 8 GB free, 10% CPU, 60 GB disk, codex installed.
fn roomy() -> HostObservation {
    HostObservation {
        capabilities: None,
        sampled_at_ms: Some(150),
        memory: MemoryCapacity {
            free_mb: Some(8192),
            total_mb: Some(16_384),
        },
        cpu: CpuCapacity {
            load_percent: Some(10),
            logical_cpus: Some(8),
        },
        disk: DiskCapacity {
            free_mb: Some(60_000),
            total_mb: Some(500_000),
        },
        tools: Some(vec![
            ToolAvailability {
                tool: "codex".into(),
                installed: true,
                configured: true,
            },
            ToolAvailability {
                tool: "claude".into(),
                installed: false,
                configured: true,
            },
        ]),
        sessions: Some(Vec::new()),
    }
}
fn probed(m: &mut Machine) -> Arc<Scripted> {
    let probe = Arc::new(Scripted::default());
    probe.set(|observation| *observation = roomy());
    m.runtime = m.runtime.clone().with_capacity_probe(probe.clone());
    probe
}
fn scope(tool: &str) -> ToolScope {
    ToolScope {
        tool: tool.into(),
        models: Vec::new(),
    }
}
/// Machine default 4 sessions; target 1 overrides the RAM floor to 3000 MB.
fn policy() -> PolicyBody {
    PolicyBody {
        machine: MachineLimits {
            max_sessions: 4,
            ..MachineLimits::default()
        },
        tools: ["codex", "claude"]
            .map(|tool| ToolPolicy {
                tool: tool.into(),
                max_sessions: 4,
                weight: 50,
                accounts: Vec::new(),
            })
            .into(),
        targets: vec![TargetPolicy {
            target_id: id(1),
            enabled: true,
            actions: None,
            tools: vec![scope("codex"), scope("claude")],
            max_sessions: 2,
            machine: Some(MachineLimits {
                max_sessions: 3,
                min_free_ram_mb: 3000,
                ..MachineLimits::default()
            }),
            speculation: None,
        }],
        ..PolicyBody::default()
    }
}
fn set_policy(
    runtime: &OrchestrationRuntime,
    owner: &str,
    body: PolicyBody,
    tx: &mut Transaction<'_>,
) {
    let expected_revision = runtime.policy(owner).unwrap().revision;
    runtime
        .set_policy(
            owner,
            SetPolicy {
                expected_revision,
                policy: body,
            },
            Some(&human()),
            tx,
        )
        .unwrap();
}
/// A person remembers codex session starts on `target` (HS2-XHQ1EH).
fn grant_sessions(runtime: &OrchestrationRuntime, target: &TargetRef, tx: &mut Transaction<'_>) {
    allow_on(
        runtime,
        &target.owner_checkout_id,
        &target.target_id,
        &[TargetAction::SessionsManage],
        &["codex"],
        tx,
    );
}
fn ask(tool: &str, sessions: u32) -> TargetAdmission {
    serde_json::from_value(json!({"tool": tool, "sessions": sessions})).unwrap()
}
fn answer(decision: &AdmissionDecision) -> (AdmissionOutcome, u32, Option<AdmissionReason>) {
    (decision.outcome, decision.admitted, decision.reason)
}

#[test]
fn hs2_j8prtv_local_target_capacity_and_admission_pass_the_gate_and_the_targets_limits() {
    use AdmissionOutcome::{Accepted, Reduced, Refused};
    let mut m = machine(10, false, unsupported);
    let probe = probed(&mut m);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[TargetAction::GatesObserve], &mut tx);
    let target = m.reference(1);
    grant_sessions(&m.runtime, &target, &mut tx);
    set_policy(&m.runtime, &m.owner.id, policy(), &mut tx);
    // Setup asked the probe nothing: a snapshot exists only because a request wants one.
    assert_eq!(probe.asked.load(Ordering::SeqCst), 0);

    let snapshot = m
        .runtime
        .target_capacity(&target, &[], Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(probe.asked.load(Ordering::SeqCst), 1);
    assert_eq!(snapshot.server_id, id(10));
    assert_eq!(snapshot.checkout_id, m.target.id);
    assert_eq!(snapshot.memory.free_mb, Some(8192));
    assert_eq!(snapshot.cpu.load_percent, Some(10));
    assert_eq!(snapshot.disk.free_mb, Some(60_000));
    assert_eq!(snapshot.sessions, Some(Vec::new()));
    // HS2-YDZAFH: no namespace named lists every held claim; none is held yet.
    assert_eq!(
        (snapshot.claims, snapshot.claim_namespaces.len()),
        (Some(Vec::new()), 0)
    );
    assert_eq!(
        m.last_audit(),
        ("gates_observe".into(), AuditOutcome::Succeeded, None)
    );
    // Held claims are reported for the namespaces the caller names.
    let claims = hotsheet_resource_claims::ClaimService::new(&m.home);
    claims
        .acquire(
            &hotsheet_resource_claims::ResourceRef::parse("builds/ios-simulator").unwrap(),
            std::time::Duration::from_secs(600),
            None,
            &std::collections::BTreeMap::new(),
        )
        .unwrap();
    let named = m
        .runtime
        .target_capacity(
            &target,
            &["builds".into(), "idle".into()],
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(named.claim_namespaces, ["builds", "idle"]);
    let held = named.claims.unwrap();
    assert_eq!(held.len(), 1);
    assert_eq!(
        (held[0].namespace.as_str(), held[0].resource.as_str()),
        ("builds", "ios-simulator")
    );
    // HS2-YDZAFH: named nowhere, the same claim is found by enumeration.
    let all = m
        .runtime
        .target_capacity(&target, &[], Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(all.claim_namespaces, ["builds"]);
    assert_eq!(all.claims.as_ref().map(Vec::len), Some(1));
    // A claim is a namespace, a resource and an expiry: never its token or metadata.
    let wire = serde_json::to_value(&held[0]).unwrap();
    let mut keys: Vec<_> = wire.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["expires_at_ms", "namespace", "resource"]);

    // Admission under the target's own policy limits (3 sessions, 3000 MB floor).
    let admit = |m: &Machine, tool: &str, sessions: u32, tx: &mut Transaction<'_>| {
        m.runtime
            .target_admission(&target, ask(tool, sessions), Some(&worker()), tx)
    };
    let accepted = admit(&m, "codex", 2, &mut tx).unwrap();
    assert_eq!(answer(&accepted), (Accepted, 2, None));
    assert_eq!(
        accepted.limits,
        MachineLimits {
            max_sessions: 3,
            min_free_ram_mb: 3000,
            ..MachineLimits::default()
        }
    );
    assert_eq!(accepted.sampled_at_ms, Some(150));
    assert_eq!(
        m.last_audit(),
        ("sessions_manage".into(), AuditOutcome::Succeeded, None)
    );
    // More than the cap is reduced to what fits.
    assert_eq!(
        answer(&admit(&m, "codex", 4, &mut tx).unwrap()),
        (Reduced, 3, Some(AdmissionReason::SessionCapReached))
    );
    // Low RAM, high load, low disk and a session already running each say so.
    probe.set(|observation| observation.memory.free_mb = Some(3000 + 1023));
    assert_eq!(
        answer(&admit(&m, "codex", 1, &mut tx).unwrap()),
        (Refused, 0, Some(AdmissionReason::BelowRamFloor))
    );
    probe.set(|observation| {
        observation.memory.free_mb = Some(8192);
        observation.cpu.load_percent = Some(97);
    });
    assert_eq!(
        answer(&admit(&m, "codex", 1, &mut tx).unwrap()),
        (Refused, 0, Some(AdmissionReason::AboveCpuCeiling))
    );
    probe.set(|observation| {
        observation.cpu.load_percent = Some(10);
        observation.disk.free_mb = Some(100);
    });
    assert_eq!(
        answer(&admit(&m, "codex", 1, &mut tx).unwrap()),
        (Refused, 0, Some(AdmissionReason::BelowDiskFloor))
    );
    probe.set(|observation| {
        observation.disk.free_mb = Some(60_000);
        observation.sessions = Some(vec![SessionFootprint {
            session_id: "term-1".into(),
            tool: Some("codex".into()),
            busy: true,
            on_checkout: Some(true),
            resident_mb: Some(2000),
        }]);
    });
    // 8192 - 3000 leaves room for two sessions of the observed 2000 MB each.
    let observed = admit(&m, "codex", 3, &mut tx).unwrap();
    assert_eq!(
        answer(&observed),
        (Reduced, 2, Some(AdmissionReason::SessionCapReached))
    );
    assert_eq!(observed.footprint_mb, 2000);
    probe.set(|observation| observation.sessions = Some(Vec::new()));

    // The machine owner's file is stricter than the policy and wins; a looser value
    // in it changes nothing.
    let local = AdmissionStore::new(&m.home);
    local
        .update(|config| {
            config.max_sessions = Some(1);
            config.max_cpu_percent = Some(100);
        })
        .unwrap();
    let stricter = admit(&m, "codex", 2, &mut tx).unwrap();
    assert_eq!(
        answer(&stricter),
        (Reduced, 1, Some(AdmissionReason::SessionCapReached))
    );
    assert_eq!(
        (
            stricter.limits.max_sessions,
            stricter.limits.max_cpu_percent
        ),
        (1, 90)
    );
    local.update(|config| config.max_sessions = None).unwrap();

    // HS2-XHQ1EH: the question starts nothing, so it asks no one, whoever asks.
    set_policy(&m.runtime, &m.owner.id, policy(), &mut tx);
    let asked = probe.asked.load(Ordering::SeqCst);
    m.runtime
        .target_admission(
            &target,
            ask("codex", 1),
            Some(&MutationActor::parse("ai", Some("stranger".into())).unwrap()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(probe.asked.load(Ordering::SeqCst), asked + 1);
    // A tool the machine lacks is the target's refusal.
    probe.set(|observation| observation.tools = Some(Vec::new()));
    assert_eq!(
        answer(&admit(&m, "codex", 1, &mut tx).unwrap()),
        (Refused, 0, Some(AdmissionReason::ToolNotInstalled))
    );

    // Unknown capacity refuses unless this machine's owner allowed it.
    probe.set(|observation| *observation = HostObservation::default());
    let unknown = admit(&m, "codex", 1, &mut tx).unwrap();
    assert_eq!(
        answer(&unknown),
        (Refused, 0, Some(AdmissionReason::CapacityUnknown))
    );
    assert_eq!(unknown.unknown.len(), 5);
    assert!(unknown.unknown.contains(&CapacityField::Memory));
    local.update(|config| config.allow_unknown = true).unwrap();
    assert_eq!(
        answer(&admit(&m, "codex", 1, &mut tx).unwrap()),
        (Accepted, 1, None)
    );
    // A damaged admission file is an error, never a looser default.
    std::fs::write(m.home.join("orchestration-admission.json"), b"{").unwrap();
    assert!(matches!(
        admit(&m, "codex", 1, &mut tx),
        Err(OrchestrationError::Corrupt)
    ));
    assert_eq!(
        m.last_audit(),
        (
            "sessions_manage".into(),
            AuditOutcome::Failed,
            Some("corrupt_orchestration_data".into())
        )
    );
    std::fs::remove_file(m.home.join("orchestration-admission.json")).unwrap();

    // The question starts nothing, so no permission request is filed for it.
    probe.set(|observation| *observation = roomy());
    let stranger = MutationActor::parse("ai", Some("stranger".into())).unwrap();
    assert_eq!(
        answer(
            &m.runtime
                .target_admission(&target, ask("codex", 1), Some(&stranger), &mut tx)
                .unwrap()
        ),
        (Accepted, 1, None)
    );
    assert!(
        m.runtime
            .permission_list(&m.owner.id, &mut tx)
            .unwrap()
            .requests
            .is_empty()
    );
    // The policy still bounds the question: a tool it does not list for the target
    // is refused before the target is asked, and the refusal is audited.
    let asked = probe.asked.load(Ordering::SeqCst);
    assert!(matches!(
        m.runtime
            .target_admission(&target, ask("unlisted-tool", 1), Some(&stranger), &mut tx),
        Err(OrchestrationError::Denied(
            hotsheet_ticketing::orchestration::DenialReason::PolicyDenied
        ))
    ));
    assert_eq!(probe.asked.load(Ordering::SeqCst), asked);
    assert_eq!(m.last_audit().1, AuditOutcome::Failed);
    // Observing capacity is consented to: a target the policy disables is still read.
    let mut disabled = policy();
    disabled.targets[0].enabled = false;
    set_policy(&m.runtime, &m.owner.id, disabled, &mut tx);
    m.runtime
        .target_capacity(&target, &[], Some(&worker()), &mut tx)
        .unwrap();
    assert!(matches!(
        m.runtime.target_capacity(&target, &[], None, &mut tx),
        Err(OrchestrationError::Invalid(_))
    ));
}

#[test]
fn hs2_j8prtv_orchestration_capacity_routes_and_hosted_routes() {
    let mut m = machine(10, false, unsupported);
    let probe = probed(&mut m);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[TargetAction::GatesObserve], &mut tx);
    grant_sessions(&m.runtime, &m.reference(1), &mut tx);
    set_policy(&m.runtime, &m.owner.id, policy(), &mut tx);
    let owner = &m.owner.id;
    let hosted = format!("/checkouts/{}", m.target.id);
    let targets = format!("/checkouts/{owner}/orchestration/targets/{}", id(1));
    let actor = json!({"role":"ai","id":"worker-1"});
    for path in [
        format!("{hosted}/target-capacity"),
        format!("{hosted}/target-admission"),
        format!("{targets}/capacity"),
        format!("{targets}/admission"),
    ] {
        assert!(is_path(&path), "{path}");
        // Only the orchestrating side is fenced to this machine; a remote orchestrator
        // must be able to reach the hosted half.
        assert_eq!(is_machine_path(&path), path.contains("/orchestration/"));
        assert!(!is_cleanup_request("GET", &path));
    }

    // Hosted: ordinary reads of this machine, no actor and no grant involved.
    let snapshot = m
        .runtime
        .request(
            "GET",
            &format!("{hosted}/target-capacity"),
            &Value::Null,
            &mut tx,
        )
        .unwrap();
    assert_eq!(snapshot["schema_version"], 1);
    assert_eq!(snapshot["checkout_id"], m.target.id.as_str());
    assert_eq!(snapshot["memory"]["free_mb"], 8192);
    // HS2-YDZAFH: no namespace named and none held on this machine: an empty list.
    assert_eq!(snapshot["claims"], json!([]));
    let named = m
        .runtime
        .request(
            "GET",
            &format!("{hosted}/target-capacity?claim_namespaces=builds%2Cbuilds,tests"),
            &Value::Null,
            &mut tx,
        )
        .unwrap();
    assert_eq!(named["claim_namespaces"], json!(["builds", "tests"]));
    assert_eq!(named["claims"], json!([]));
    for query in [
        "Bad_Name",
        "a,b,c,d,e,f,g,h,i",
        "..",
        "builds&claim_namespaces=x",
    ] {
        assert!(matches!(
            m.runtime.request(
                "GET",
                &format!("{hosted}/target-capacity?claim_namespaces={query}"),
                &Value::Null,
                &mut tx
            ),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    let decided = m
        .runtime
        .request(
            "POST",
            &format!("{hosted}/target-admission"),
            &json!({"tool":"codex","sessions":3,"limits":{"max_sessions":2},"actor":actor}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(decided["outcome"], "reduced");
    assert_eq!(decided["admitted"], 2);
    assert_eq!(decided["reason"], "session_cap_reached");
    assert_eq!(decided["limits"]["min_free_ram_mb"], 2048);
    // Without limits the policy defaults stand in.
    let bare = m
        .runtime
        .request(
            "POST",
            &format!("{hosted}/target-admission"),
            &json!({"tool":"codex"}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        (&bare["outcome"], &bare["requested"]),
        (&json!("accepted"), &json!(1))
    );
    assert_eq!(
        bare["limits"],
        serde_json::to_value(MachineLimits::default()).unwrap()
    );
    for body in [
        json!({}),
        json!({"tool":"codex","sessions":0}),
        json!({"tool":"codex","sessions":65}),
        json!({"tool":"co dex"}),
        json!({"tool":"codex","limits":{"max_cpu_percent":0}}),
        json!({"tool":"codex","path":"/synthetic"}),
    ] {
        assert!(
            matches!(
                m.runtime.request(
                    "POST",
                    &format!("{hosted}/target-admission"),
                    &body,
                    &mut tx
                ),
                Err(OrchestrationError::Invalid(_))
            ),
            "{body}"
        );
    }
    // An unregistered checkout is not this machine's to answer for.
    assert!(
        m.runtime
            .request(
                "GET",
                "/checkouts/absent-0123456789ab/target-capacity",
                &Value::Null,
                &mut tx
            )
            .is_err()
    );

    // Orchestrating: the read takes its actor in the query, or in a body.
    let asked = probe.asked.load(Ordering::SeqCst);
    let read = m
        .runtime
        .request(
            "GET",
            &format!("{targets}/capacity?actor_id=worker-1&actor_role=ai&claim_namespaces=builds"),
            &Value::Null,
            &mut tx,
        )
        .unwrap();
    assert_eq!(read["claim_namespaces"], json!(["builds"]));
    let posted = m
        .runtime
        .request(
            "POST",
            &format!("{targets}/capacity"),
            &json!({"actor":actor,"claim_namespaces":["builds"]}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(read, posted);
    assert_eq!(probe.asked.load(Ordering::SeqCst), asked + 2);
    assert!(matches!(
        m.runtime
            .request("GET", &format!("{targets}/capacity"), &Value::Null, &mut tx),
        Err(OrchestrationError::Invalid(_))
    ));
    // Reading is consented to for any identified actor.
    m.runtime
        .request(
            "GET",
            &format!("{targets}/capacity?actor_id=stranger"),
            &Value::Null,
            &mut tx,
        )
        .unwrap();
    let admitted = m
        .runtime
        .request(
            "POST",
            &format!("{targets}/admission"),
            &json!({"actor":actor,"tool":"codex","model":null,"sessions":null}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(admitted["outcome"], "accepted");
    assert_eq!(admitted["limits"]["max_sessions"], 3);
    // The orchestrator cannot send its own limits past the policy.
    assert!(matches!(
        m.runtime.request(
            "POST",
            &format!("{targets}/admission"),
            &json!({"actor":actor,"tool":"codex","limits":{"max_sessions":64}}),
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
}

#[test]
fn hs2_j8prtv_remote_target_answers_from_its_own_machine_over_the_reviewed_transport() {
    // The peer machine hosts the target and measures itself.
    let mut hosted = machine(99, false, unsupported);
    let probe = probed(&mut hosted);
    probe.set(|observation| observation.memory.free_mb = Some(6000));
    let peer = Arc::new(RemotePeer {
        hosted,
        calls: Mutex::new(Vec::new()),
        status_override: Mutex::new(None),
    });
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let project = root.path().join("owner");
    std::fs::create_dir(&project).unwrap();
    let store_path = root.path().join("owner-store");
    hotsheet_ticketing::FsStore::init_git(
        &store_path,
        &hotsheet_ticketing::StoreMetadata::new("OWN"),
    )
    .unwrap();
    let checkouts = CheckoutRegistry::new(home.join("checkouts.json"));
    let owner = checkouts
        .register(&project, None, None, vec![store_path])
        .unwrap();
    let secrets = Arc::new(Memory::default());
    // The controller has no probe of its own: every value it reports is the peer's.
    let runtime = OrchestrationRuntime::new(
        home.clone(),
        checkouts,
        ProviderConfigRegistry::new(root.path().join("providers.json")),
        secrets.clone(),
        peer.clone(),
        id(10),
    )
    .unwrap();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
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
                    label: "Remote target".into(),
                    expected_repository_identity: None,
                    site_of: None,
                }],
            },
            &mut tx,
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
    assert!(
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
                &mut tx
            )
            .unwrap()
            .usable
    );
    grant_sessions(&runtime, &target, &mut tx);
    set_policy(&runtime, &owner.id, policy(), &mut tx);
    let calls = || {
        std::mem::take(
            &mut *peer
                .calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    };
    calls();

    let snapshot: CapacitySnapshot = runtime
        .target_capacity(&target, &["builds".into()], Some(&worker()), &mut tx)
        .unwrap();
    // The peer's identity and the peer's numbers, read once on the peer.
    assert_eq!(snapshot.server_id, id(99));
    assert_eq!(snapshot.checkout_id, peer.hosted.target.id);
    assert_eq!(snapshot.memory.free_mb, Some(6000));
    assert_eq!(snapshot.claims, Some(Vec::new()));
    assert_eq!(probe.asked.load(Ordering::SeqCst), 1);
    assert_eq!(
        calls(),
        [(
            "GET".to_owned(),
            format!(
                "/checkouts/{}/target-capacity?claim_namespaces=builds",
                peer.hosted.target.id
            )
        )]
    );

    // 6000 free - 3000 floor leaves room for two default 1024 MB sessions.
    let reduced = runtime
        .target_admission(&target, ask("codex", 3), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(
        answer(&reduced),
        (
            AdmissionOutcome::Reduced,
            2,
            Some(AdmissionReason::BelowRamFloor)
        )
    );
    assert_eq!(reduced.limits.min_free_ram_mb, 3000);
    assert_eq!(
        calls(),
        [(
            "POST".to_owned(),
            format!("/checkouts/{}/target-admission", peer.hosted.target.id)
        )]
    );
    // The limits that count are the target machine's: its owner's file, not the
    // controller's, makes the answer stricter.
    AdmissionStore::new(&home)
        .update(|config| config.max_sessions = Some(0))
        .unwrap();
    assert_eq!(
        runtime
            .target_admission(&target, ask("codex", 1), Some(&worker()), &mut tx)
            .unwrap()
            .outcome,
        AdmissionOutcome::Accepted
    );
    AdmissionStore::new(&peer.hosted.home)
        .update(|config| config.max_sessions = Some(0))
        .unwrap();
    let refused = runtime
        .target_admission(&target, ask("codex", 1), Some(&worker()), &mut tx)
        .unwrap();
    assert_eq!(
        answer(&refused),
        (
            AdmissionOutcome::Refused,
            0,
            Some(AdmissionReason::SessionCapReached)
        )
    );
    assert_eq!(refused.limits.max_sessions, 0);
    // The peer's damaged file fails closed as a failed target operation.
    std::fs::write(peer.hosted.home.join("orchestration-admission.json"), b"{").unwrap();
    assert_eq!(
        op(runtime.target_admission(&target, ask("codex", 1), Some(&worker()), &mut tx)),
        TargetOpError::Failed
    );
    // A peer that predates these routes says so by status alone.
    *peer
        .status_override
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(404);
    assert_eq!(
        op(runtime.target_capacity(&target, &[], Some(&worker()), &mut tx)),
        TargetOpError::PeerUnsupported
    );
    assert_eq!(
        op(runtime.target_admission(&target, ask("codex", 1), Some(&worker()), &mut tx)),
        TargetOpError::PeerUnsupported
    );
    let journal = runtime.audit(&owner.id, 0, 500).unwrap();
    let last = &journal.entries.last().unwrap().record;
    assert_eq!(
        (last.action.as_str(), last.outcome, last.reason.as_deref()),
        (
            "sessions_manage",
            AuditOutcome::Failed,
            Some("target_peer_unsupported")
        )
    );
}

#[test]
fn hs2_j8prtv_a_runtime_without_a_session_host_reports_unknown_and_fails_closed() {
    // No probe injected: this process hosts no sessions and knows nothing.
    let m = machine(10, false, unsupported);
    let snapshot = m.runtime.hosted_capacity(&m.target.id, &[]).unwrap();
    assert_eq!(snapshot.sampled_at_ms, None);
    assert_eq!(
        (snapshot.memory.free_mb, snapshot.cpu.load_percent),
        (None, None)
    );
    assert_eq!((snapshot.tools, snapshot.sessions), (None, None));
    let decision = m
        .runtime
        .hosted_admission(
            &m.target.id,
            serde_json::from_value(json!({"tool":"codex"})).unwrap(),
        )
        .unwrap();
    assert_eq!(
        answer(&decision),
        (
            AdmissionOutcome::Refused,
            0,
            Some(AdmissionReason::CapacityUnknown)
        )
    );
    // A headless process finds no live server record under an empty machine home.
    let headless = LocalServerCapacity::new(&m.home);
    assert_eq!(headless.observe(&m.target), HostObservation::default());
    std::fs::create_dir_all(m.home.join("instances")).unwrap();
    std::fs::write(m.home.join("instances/stale.json"), b"{not json").unwrap();
    assert_eq!(headless.observe(&m.target), HostObservation::default());
}

/// HS2-P5GTSW: a capacity subscriber waits on the host instead of asking again.
#[test]
fn hs2_p5gtsw_a_capacity_wait_samples_only_while_a_subscriber_waits() {
    let mut m = machine(10, false, unsupported);
    let probe = probed(&mut m);
    m.runtime = m
        .runtime
        .clone()
        .with_capacity_resample(std::time::Duration::from_millis(40));
    let checkout = m.target.id.clone();
    let asked = || probe.asked.load(Ordering::SeqCst);
    // Nobody subscribes: nothing is sampled.
    assert_eq!(asked(), 0);
    // A plain read samples once and carries the version a subscriber passes back.
    let first = m
        .runtime
        .hosted_capacity_wait(&checkout, &[], None, 5_000)
        .unwrap();
    assert_eq!(asked(), 1);
    let version = first.version.clone().unwrap();
    assert_eq!(version.len(), 16);
    // A version that is not current answers at once (a reconnect after a change).
    let stale = m
        .runtime
        .hosted_capacity_wait(&checkout, &[], Some("0000000000000000"), 5_000)
        .unwrap();
    assert_eq!(stale.version.as_deref(), Some(version.as_str()));
    assert_eq!(asked(), 2);
    // Subscribed to the current version: it samples while it waits, and answers the
    // same version at the deadline when nothing changed.
    let began = std::time::Instant::now();
    let same = m
        .runtime
        .hosted_capacity_wait(&checkout, &[], Some(&version), 300)
        .unwrap();
    assert!(began.elapsed() >= std::time::Duration::from_millis(300));
    assert_eq!(same.version.as_deref(), Some(version.as_str()));
    let during = asked() - 2;
    assert!((2..=12).contains(&during), "{during}");
    // Jitter inside a step is not a change.
    probe.set(|observation| {
        observation.memory.free_mb = Some(8_200);
        observation.cpu.load_percent = Some(15);
        observation.sampled_at_ms = Some(999);
    });
    assert_eq!(
        m.runtime
            .hosted_capacity_wait(&checkout, &[], Some(&version), 100)
            .unwrap()
            .version,
        Some(version.clone())
    );
    // A change while it waits answers early with the new version.
    let changer = probe.clone();
    let change = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        changer.set(|observation| observation.memory.free_mb = Some(1_000));
    });
    let began = std::time::Instant::now();
    let changed = m
        .runtime
        .hosted_capacity_wait(&checkout, &[], Some(&version), 5_000)
        .unwrap();
    change.join().unwrap();
    assert!(began.elapsed() < std::time::Duration::from_millis(2_000));
    assert_ne!(changed.version.as_deref(), Some(version.as_str()));
    // After the subscriber leaves nothing samples.
    let settled = asked();
    std::thread::yield_now();
    assert_eq!(asked(), settled);
    // The same capacity reads as the same version from a new process.
    let mut again = machine(10, false, unsupported);
    let other = probed(&mut again);
    other.set(|observation| observation.memory.free_mb = Some(1_000));
    assert_eq!(
        again
            .runtime
            .hosted_capacity(&again.target.id, &[])
            .unwrap()
            .version,
        changed.version
    );
    // The version a subscriber passes is bounded.
    for bad in ["", "not hex!", &"a".repeat(65)] {
        assert!(matches!(
            m.runtime.hosted_capacity_wait(&checkout, &[], Some(bad), 0),
            Err(OrchestrationError::Invalid(_))
        ));
    }
}

#[test]
fn hs2_p5gtsw_the_capacity_routes_wait_by_version_locally_and_over_the_transport() {
    let mut m = machine(10, false, unsupported);
    let probe = probed(&mut m);
    m.runtime = m
        .runtime
        .clone()
        .with_capacity_resample(std::time::Duration::from_millis(20));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[], &mut tx);
    let hosted = format!("/checkouts/{}/target-capacity", m.target.id);
    let version = m.runtime.request_now("GET", &hosted, &Value::Null).unwrap()["version"]
        .as_str()
        .unwrap()
        .to_owned();
    let waited = m
        .runtime
        .request_now(
            "GET",
            &format!("{hosted}?after={version}&wait_ms=100"),
            &Value::Null,
        )
        .unwrap();
    assert_eq!(waited["version"], version.as_str());
    // The wait is bounded by the hosted half whatever the caller asks.
    assert!(
        m.runtime
            .request_now(
                "GET",
                &format!("{hosted}?after=bad%20one&wait_ms=100"),
                &Value::Null,
            )
            .is_err()
    );
    // The orchestrating read passes the same wait through the gate (observation never
    // asks) and answers the new version once the host changes.
    let owner_route = format!(
        "/checkouts/{}/orchestration/targets/{}/capacity",
        m.owner.id,
        id(1)
    );
    let changer = probe.clone();
    let change = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(80));
        changer.set(|observation| observation.tools = None);
    });
    let read = m
        .runtime
        .request(
            "POST",
            &owner_route,
            &json!({"after": version, "wait_ms": 5_000,
                "actor": {"role": "ai", "id": "worker-1"}}),
            &mut tx,
        )
        .unwrap();
    change.join().unwrap();
    assert_ne!(read["version"], version.as_str());
    assert!(read["tools"].is_null());
    assert_eq!(
        m.last_audit(),
        ("gates_observe".into(), AuditOutcome::Succeeded, None)
    );
    // The query a remote host receives carries the wait.
    assert_eq!(
        super::super::capacity::capacity_query(&["vm".into()], Some("ab12"), 60_000),
        format!("?claim_namespaces=vm&after=ab12&wait_ms={MAX_CAPACITY_WAIT_MS}")
    );
    assert_eq!(super::super::capacity::capacity_query(&[], None, 5), "");
}

/// HS2-YDZAFH: a capacity read that names no namespace enumerates them. A claim source
/// that cannot enumerate, or more namespaces than the bound, reads as unknown rather
/// than a partial list; a named read never enumerates.
#[test]
fn hs2_ydzafh_an_unnamed_capacity_read_lists_every_held_claim_or_none_at_all() {
    use crate::orchestration::capacity::MAX_ENUMERATED_NAMESPACES;
    use crate::orchestration::resources::{ClaimSource, ClaimsUnreachable};
    use hotsheet_ticketing::orchestration::HeldClaim;
    struct Source {
        names: Option<usize>,
        listed: Mutex<Vec<String>>,
    }
    impl ClaimSource for Source {
        fn held(&self, namespace: &str) -> Result<Vec<HeldClaim>, ClaimsUnreachable> {
            self.listed.lock().unwrap().push(namespace.to_owned());
            Ok(vec![HeldClaim {
                namespace: namespace.to_owned(),
                resource: "r".into(),
                expires_at_ms: 1,
            }])
        }
        fn wait_free(&self, _: &str, _: &str, _: std::time::Duration) -> bool {
            true
        }
        fn namespaces(&self) -> Result<Vec<String>, ClaimsUnreachable> {
            match self.names {
                Some(count) => Ok((0..count).map(|n| format!("ns{n}")).collect()),
                None => Err(ClaimsUnreachable {
                    offline: hotsheet_ticketing::orchestration::OfflinePolicy::FailClosed,
                }),
            }
        }
    }
    let read = |names: Option<usize>, named: &[String]| {
        let mut m = machine(10, false, unsupported);
        let source = Arc::new(Source {
            names,
            listed: Mutex::new(Vec::new()),
        });
        m.runtime = m.runtime.clone().with_claim_source(source.clone());
        let snapshot = m.runtime.hosted_capacity(&m.target.id, named).unwrap();
        let listed = source.listed.lock().unwrap().clone();
        (snapshot, listed)
    };
    let (two, listed) = read(Some(2), &[]);
    assert_eq!(two.claim_namespaces, ["ns0", "ns1"]);
    assert_eq!(two.claims.map(|claims| claims.len()), Some(2));
    assert_eq!(listed, ["ns0", "ns1"]);
    // At the bound, still listed; one past it, unknown and nothing listed.
    let (bound, _) = read(Some(MAX_ENUMERATED_NAMESPACES), &[]);
    assert_eq!(
        bound.claims.map(|claims| claims.len()),
        Some(MAX_ENUMERATED_NAMESPACES)
    );
    let (over, listed) = read(Some(MAX_ENUMERATED_NAMESPACES + 1), &[]);
    assert_eq!((over.claims, over.claims_offline), (None, None));
    assert!(listed.is_empty());
    // A source that cannot enumerate: unknown, as before enumeration existed.
    let (unknown, _) = read(None, &[]);
    assert_eq!((unknown.claims, unknown.claim_namespaces.len()), (None, 0));
    // Named: exactly the named ones, enumeration unused.
    let (named, listed) = read(None, &["builds".into()]);
    assert_eq!(named.claim_namespaces, ["builds"]);
    assert_eq!(listed, ["builds"]);
    // Nothing on the machine: an empty list, not unknown.
    let (empty, _) = read(Some(0), &[]);
    assert_eq!(empty.claims, Some(Vec::new()));
}
