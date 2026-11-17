//! HS2-AZ15GA: target tickets through the shared runtime, for local git sources, an
//! external source and a remote destination. Synthetic stores and identities only.
use std::sync::atomic::AtomicBool;

use hotsheet_model::{OrchestrationLink, OrchestrationRelation as Relation, Ticket};
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AuditOutcome, DenialReason, PermissionAnswer, TargetAction, TargetOpError,
};
use hotsheet_ticketing::remote_servers::TargetReply;
use hotsheet_ticketing::{FsStore, NewTicket, ProviderError, StoreMetadata, ops};
use serde_json::{Value, json};

use super::*;

pub(super) const NOW: &str = "2026-10-15T00:00:00Z";

pub(super) fn human() -> MutationActor {
    MutationActor::parse("human", Some("maintainer".into())).unwrap()
}
pub(super) fn worker() -> MutationActor {
    MutationActor::parse("ai", Some("worker-1".into())).unwrap()
}

pub(super) struct Refusing {
    pub(super) opened: AtomicBool,
    error: fn(&str) -> ProviderError,
}
impl ExternalProviders for Refusing {
    fn open(
        &self,
        _: &ProviderConfigRegistry,
        connection: &ProviderConnection,
    ) -> Result<Arc<dyn hotsheet_ticketing::TicketProvider>, ProviderError> {
        self.opened.store(true, Ordering::SeqCst);
        Err((self.error)(&connection.id))
    }
}

/// One machine: an owner checkout with a git ticket store, a target checkout with a
/// git ticket store and (optionally) an external source.
pub(super) struct Machine {
    pub(super) _root: tempfile::TempDir,
    pub(super) home: PathBuf,
    pub(super) runtime: OrchestrationRuntime,
    pub(super) owner: Checkout,
    pub(super) owner_store: FsStore,
    pub(super) target: Checkout,
    pub(super) target_store: FsStore,
    pub(super) external: Arc<Refusing>,
}
/// HS2-PJYNND: a test's hosted validation steps die with its machine, on every exit
/// path including a panic. Every step group still recorded in this machine's home whose
/// leader is still the recorded process is killed, so no step loop outlives the test
/// (its checkout is about to be removed, so a loop waiting on a file there never ends).
impl Drop for Machine {
    fn drop(&mut self) {
        stop_recorded_steps(&self.home);
    }
}

/// Kill every step group recorded in `home` whose leader still has its recorded start
/// time (never a reused pid). Returns how many were signalled.
pub(super) fn stop_recorded_steps(home: &Path) -> usize {
    let registry = hotsheet_ticketing::orchestration::HostedValidationRegistry::new(home);
    let mut stopped = 0;
    for record in registry.with_process().unwrap_or_default() {
        let Some(process) = record.process else {
            continue;
        };
        if crate::orchestration::step_process::process_started(process.pid) != Some(process.started)
        {
            continue;
        }
        #[cfg(unix)]
        if let Ok(group) = libc::pid_t::try_from(process.pid) {
            // SAFETY: plain integers; the group's leader is the recorded step process,
            // checked by its start time just above.
            if unsafe { libc::killpg(group, libc::SIGKILL) } == 0 {
                stopped += 1;
            }
        }
    }
    stopped
}

impl Machine {
    pub(super) fn owner_connection(&self) -> String {
        self.owner.sources[0].connection_id.clone()
    }
    pub(super) fn target_connection(&self) -> String {
        self.target
            .sources
            .iter()
            .find(|source| source.provider == "git")
            .unwrap()
            .connection_id
            .clone()
    }
    pub(super) fn orchestration_ticket(&self, title: &str) -> Ticket {
        ops::create(
            &self.owner_store,
            ulid::Ulid::new(),
            "OWN",
            NOW.into(),
            NewTicket {
                title: title.into(),
                ..Default::default()
            },
        )
        .unwrap()
    }
    pub(super) fn reference(&self, n: u128) -> TargetRef {
        TargetRef {
            owner_checkout_id: self.owner.id.clone(),
            target_id: id(n),
        }
    }
    pub(super) fn links(&self, ticket: &Ticket) -> Vec<OrchestrationLink> {
        self.owner_store
            .read_ticket(&ticket.id)
            .unwrap()
            .orchestration_links
    }
    pub(super) fn last_audit(&self) -> (String, AuditOutcome, Option<String>) {
        let entry = self
            .runtime
            .audit(&self.owner.id, 0, 500)
            .unwrap()
            .entries
            .pop()
            .unwrap();
        (
            entry.record.action,
            entry.record.outcome,
            entry.record.reason,
        )
    }
}

pub(super) fn machine(
    server: u128,
    with_external: bool,
    error: fn(&str) -> ProviderError,
) -> Machine {
    machine_with(server, with_external, error, None)
}

/// HS2-7QHWHJ: the same machine with a live ticket-change feed.
pub(super) fn machine_with(
    server: u128,
    with_external: bool,
    error: fn(&str) -> ProviderError,
    events: Option<Arc<dyn TargetEvents>>,
) -> Machine {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let checkouts = CheckoutRegistry::new(home.join("checkouts.json"));
    let providers = ProviderConfigRegistry::new(root.path().join("providers.json"));
    providers
        .save(&[ProviderConnection {
            id: "external-source".into(),
            provider: "github".into(),
            locator: "synthetic/repo".into(),
            name: None,
            default: false,
            settings: Value::Null,
            disabled: false,
        }])
        .unwrap();
    let mut made = Vec::new();
    for (name, prefix) in [("owner", "OWN"), ("target", "TGT")] {
        let project = root.path().join(name);
        std::fs::create_dir(&project).unwrap();
        let store_path = root.path().join(format!("{name}-store"));
        let store = FsStore::init_git(&store_path, &StoreMetadata::new(prefix)).unwrap();
        let checkout = checkouts
            .register(&project, None, None, vec![store_path])
            .unwrap();
        made.push((checkout, store));
    }
    let (target, target_store) = made.pop().unwrap();
    let (owner, owner_store) = made.pop().unwrap();
    let target = if with_external {
        checkouts
            .add_registered_source(
                &providers,
                &target.id,
                hotsheet_ticketing::checkouts::TicketSource {
                    connection_id: "external-source".into(),
                    provider: "github".into(),
                    locator: "synthetic/repo".into(),
                },
                false,
            )
            .unwrap()
    } else {
        target
    };
    let external = Arc::new(Refusing {
        opened: AtomicBool::new(false),
        error,
    });
    let runtime = OrchestrationRuntime::new(
        home.clone(),
        checkouts,
        providers,
        Arc::new(Memory::default()),
        Arc::new(Peer::default()),
        id(server),
    )
    .unwrap()
    .with_external_providers(external.clone());
    let runtime = match events {
        Some(events) => runtime.with_target_events(events),
        None => runtime,
    };
    Machine {
        _root: root,
        home,
        runtime,
        owner,
        owner_store,
        target,
        target_store,
        external,
    }
}

pub(super) fn unsupported(connection: &str) -> ProviderError {
    ProviderError::Unsupported {
        connection_id: connection.into(),
        capability: "create",
    }
}

/// Declare target `n`, bind it to the local target checkout and remember a rule for
/// each of `actions` that would ask a person (HS2-XHQ1EH).
pub(super) fn bind_and_allow(
    m: &Machine,
    n: u128,
    actions: &[TargetAction],
    tx: &mut Transaction<'_>,
) {
    let before = m.runtime.declarations(&m.owner.id).unwrap();
    let mut targets = before.targets;
    targets.push(TargetDeclaration {
        target_id: id(n),
        state: DeclarationState::Active,
        label: "Target".into(),
        expected_repository_identity: None,
    });
    let manifest = m
        .runtime
        .replace(
            &m.owner.id,
            ReplaceTargets {
                expected_revision: before.revision,
                targets,
            },
            tx,
        )
        .unwrap();
    let projection = m.runtime.checkout_projection(&m.target.id).unwrap();
    assert!(
        m.runtime
            .bind(
                BindTarget {
                    target: m.reference(n),
                    expected_manifest_revision: manifest.revision.unwrap(),
                    expected_binding_revision: 0,
                    previous_destination: None,
                    destination: TargetDestination::Local {
                        server_id: projection.server_id,
                        checkout_id: projection.checkout_id,
                        checkout_authority: projection.checkout_authority,
                    },
                },
                tx
            )
            .unwrap()
            .usable
    );
    allow_on(&m.runtime, &m.owner.id, &id(n), actions, &[], tx);
}

/// HS2-XHQ1EH: unbind target `n` (what makes it unusable now that no grant can be
/// withdrawn), and bind it again to the same local checkout.
pub(super) fn unbind(m: &Machine, n: u128, tx: &mut Transaction<'_>) {
    let binding = OrchestrationRuntime::binding_at(&m.home, &m.reference(n)).unwrap();
    OrchestrationRuntime::unbind_at(
        &m.home,
        hotsheet_ticketing::orchestration::UnbindTarget {
            target: m.reference(n),
            expected_binding_revision: binding.revision,
            previous_destination: binding.destination,
        },
        tx,
    )
    .unwrap();
}
pub(super) fn rebind(m: &Machine, n: u128, tx: &mut Transaction<'_>) {
    let binding = OrchestrationRuntime::binding_at(&m.home, &m.reference(n)).unwrap();
    let projection = m.runtime.checkout_projection(&m.target.id).unwrap();
    let manifest = m
        .runtime
        .declarations(&m.owner.id)
        .unwrap()
        .revision
        .unwrap();
    assert!(
        m.runtime
            .bind(
                BindTarget {
                    target: m.reference(n),
                    expected_manifest_revision: manifest,
                    expected_binding_revision: binding.revision,
                    previous_destination: None,
                    destination: TargetDestination::Local {
                        server_id: projection.server_id,
                        checkout_id: projection.checkout_id,
                        checkout_authority: projection.checkout_authority,
                    },
                },
                tx
            )
            .unwrap()
            .usable
    );
}

fn create_request(
    m: &Machine,
    ticket: &Ticket,
    operation: &str,
    title: &str,
) -> CreateTargetTicket {
    serde_json::from_value(json!({
        "ticket": {"connection_id": m.owner_connection(), "native_id": ticket.id.to_string()},
        "target_id": id(1),
        "connection_id": m.target_connection(),
        "operation_id": operation,
        "title": title,
        "details": "Synthetic details",
        "tags": ["orchestrated"],
    }))
    .unwrap()
}

pub(super) fn op(result: Result<impl Sized, OrchestrationError>) -> TargetOpError {
    match result {
        Err(OrchestrationError::TargetOperation(error)) => error,
        Err(other) => panic!("expected a target operation error, got {other}"),
        Ok(_) => panic!("expected a target operation error"),
    }
}

#[test]
fn hs2_az15ga_create_is_idempotent_linked_and_carries_provenance() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsRead, TargetAction::TicketsWrite],
        &mut tx,
    );
    let ticket = m.orchestration_ticket("Coordinate the release");
    let created = m
        .runtime
        .target_ticket_create(
            &m.owner.id,
            create_request(&m, &ticket, "op-1", "Do the target work"),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert!(created.created);
    assert_eq!(
        created.link,
        LinkOutcome {
            recorded: true,
            error: None
        }
    );
    let native_id = created.ticket["native_id"].as_str().unwrap().to_owned();
    assert_eq!(created.ticket["title"], "Do the target work");
    assert_eq!(created.ticket["connection_id"], m.target_connection());
    // The target's own store holds the ticket, with the orchestration ticket as origin.
    let stored = m.target_store.list_tickets().unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].id.to_string(), native_id);
    assert_eq!(stored[0].tags, ["orchestrated"]);
    assert_eq!(stored[0].transfer_operation_id.as_deref(), Some("op-1"));
    assert_eq!(
        stored[0].transferred_from,
        Some(format!("{}:{}", m.owner_connection(), ticket.id))
    );
    assert_eq!(
        stored[0].created_by.as_ref().unwrap().id.as_deref(),
        Some("worker-1")
    );
    // The orchestration ticket holds one structured link and nothing machine-local.
    let expected = OrchestrationLink {
        target_id: id(1),
        connection_id: m.target_connection(),
        native_id: Some(native_id.clone()),
        relation: Relation::Created,
        operation_id: Some("op-1".into()),
    };
    assert_eq!(m.links(&ticket), vec![expected.clone()]);
    let file = std::fs::read_to_string(m.owner_store.ticket_path(&ticket.id)).unwrap();
    assert!(!file.contains(m.target.root.as_str()));
    assert!(!file.contains(&m.target.id));
    assert_eq!(
        m.last_audit(),
        ("tickets_write".into(), AuditOutcome::Succeeded, None)
    );
    // A retry with the retained operation id returns the same ticket and changes
    // nothing, even with different wording.
    let owner_bytes = std::fs::read(m.owner_store.ticket_path(&ticket.id)).unwrap();
    let retried = m
        .runtime
        .target_ticket_create(
            &m.owner.id,
            create_request(&m, &ticket, "op-1", "Different words on retry"),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert!(!retried.created);
    assert_eq!(retried.ticket["native_id"], native_id);
    assert_eq!(retried.ticket["title"], "Do the target work");
    assert_eq!(m.target_store.list_tickets().unwrap().len(), 1);
    assert_eq!(
        std::fs::read(m.owner_store.ticket_path(&ticket.id)).unwrap(),
        owner_bytes
    );
    // The same operation id for another orchestration ticket conflicts, creates
    // nothing and leaves no dangling intent behind.
    let other = m.orchestration_ticket("A different plan");
    assert_eq!(
        op(m.runtime.target_ticket_create(
            &m.owner.id,
            create_request(&m, &other, "op-1", "Collision"),
            Some(&worker()),
            &mut tx,
        )),
        TargetOpError::Conflict
    );
    assert_eq!(
        m.last_audit(),
        (
            "tickets_write".into(),
            AuditOutcome::Failed,
            Some("target_conflict".into())
        )
    );
    assert!(m.links(&other).is_empty());
    assert_eq!(m.target_store.list_tickets().unwrap().len(), 1);
    // A new operation id is a new ticket and a second link.
    let second = m
        .runtime
        .target_ticket_create(
            &m.owner.id,
            create_request(&m, &ticket, "op-2", "More target work"),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert!(second.created);
    assert_eq!(m.links(&ticket).len(), 2);
    assert_eq!(m.target_store.list_tickets().unwrap().len(), 2);
    // Resolved links read the target tickets and confirm the backlink.
    let views = m
        .runtime
        .target_ticket_links(
            &m.owner.id,
            ListLinks {
                ticket: OrchestrationTicket {
                    connection_id: String::new(),
                    native_id: ticket.slug.clone(),
                },
                resolve: true,
            },
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(views.len(), 2);
    assert_eq!(views[0].link, expected);
    assert_eq!(views[0].state, "resolved");
    assert_eq!(views[0].backlink, Some(true));
    assert_eq!(
        views[0].summary.as_ref().unwrap()["title"],
        "Do the target work"
    );
}

#[test]
fn hs2_az15ga_an_interrupted_create_resumes_without_a_duplicate() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsRead, TargetAction::TicketsWrite],
        &mut tx,
    );
    let ticket = m.orchestration_ticket("Coordinate");
    let planned = OrchestrationLink {
        target_id: id(1),
        connection_id: m.target_connection(),
        native_id: None,
        relation: Relation::Planned,
        operation_id: Some("op-1".into()),
    };
    let listing = |m: &Machine, tx: &mut Transaction<'_>| {
        m.runtime
            .target_ticket_links(
                &m.owner.id,
                ListLinks {
                    ticket: OrchestrationTicket {
                        connection_id: m.owner_connection(),
                        native_id: ticket.id.to_string(),
                    },
                    resolve: true,
                },
                Some(&worker()),
                tx,
            )
            .unwrap()
    };
    // Stopped after recording the intent: the link is planned and resolves to nothing.
    ops::upsert_orchestration_link(&m.owner_store, &ticket.id, planned.clone(), NOW.into())
        .unwrap();
    let before = listing(&m, &mut tx);
    assert_eq!(before[0].state, "unresolved");
    assert_eq!(before[0].reason.as_deref(), Some("target_ticket_not_found"));
    // Stopped after the target create but before the link was advanced: the planned
    // link finds the ticket by its operation id.
    let hosted = m
        .runtime
        .hosted_ticket_create(
            &m.target.id,
            HostedCreate {
                connection_id: m.target_connection(),
                operation_id: "op-1".into(),
                source: hotsheet_ticketing::TicketRef {
                    connection_id: m.owner_connection(),
                    native_id: ticket.id.to_string(),
                },
                fields: TargetTicketFields {
                    title: Some("Target work".into()),
                    ..TargetTicketFields::default()
                },
                actor: None,
            },
            &mut tx,
        )
        .unwrap();
    assert!(hosted.created);
    assert_eq!(m.links(&ticket), vec![planned.clone()]);
    let midway = listing(&m, &mut tx);
    assert_eq!(midway[0].state, "resolved");
    assert_eq!(midway[0].link, planned);
    assert_eq!(midway[0].backlink, Some(true));
    // Repeating the original request finishes the sequence with the existing ticket.
    let resumed = m
        .runtime
        .target_ticket_create(
            &m.owner.id,
            create_request(&m, &ticket, "op-1", "Target work"),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert!(!resumed.created);
    assert_eq!(resumed.ticket["native_id"], hosted.ticket["native_id"]);
    assert_eq!(m.target_store.list_tickets().unwrap().len(), 1);
    let links = m.links(&ticket);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].relation, Relation::Created);
    assert_eq!(
        links[0].native_id.as_deref(),
        hosted.ticket["native_id"].as_str()
    );
    // The hosted lookup by operation id answers null for an unknown operation.
    assert!(
        m.runtime
            .hosted_ticket_by_operation(&m.target.id, &m.target_connection(), "op-none")
            .unwrap()
            .is_none()
    );
}

#[test]
fn hs2_az15ga_every_operation_passes_the_gate_and_reports_target_errors() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[], &mut tx);
    let ticket = m.orchestration_ticket("Coordinate");
    let existing = ops::create(
        &m.target_store,
        ulid::Ulid::new(),
        "TGT",
        NOW.into(),
        NewTicket {
            title: "Already in the target".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let target = |native: &str| TargetTicket {
        target_id: id(1),
        connection_id: m.target_connection(),
        native_id: native.into(),
    };
    let read = |m: &Machine, native: &str, tx: &mut Transaction<'_>| {
        m.runtime.target_ticket_read(
            &m.owner.id,
            ReadTargetTicket {
                target: target(native),
            },
            Some(&worker()),
            tx,
        )
    };
    let update = |m: &Machine, native: &str, fields: Value, tx: &mut Transaction<'_>| {
        let mut body = json!({"target_id":id(1),"connection_id":m.target_connection(),
            "native_id":native,"operation_id":"op-update"});
        body.as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        m.runtime.target_ticket_update(
            &m.owner.id,
            serde_json::from_value(body).unwrap(),
            Some(&worker()),
            tx,
        )
    };
    // HS2-XHQ1EH: bound is consented to. An AI's write asks a person and writes
    // nothing until one answers.
    let asked = permission_request(m.runtime.target_ticket_create(
        &m.owner.id,
        create_request(&m, &ticket, "op-1", "Refused"),
        Some(&worker()),
        &mut tx,
    ));
    assert!(m.links(&ticket).is_empty());
    assert_eq!(m.target_store.list_tickets().unwrap().len(), 1);
    // A denial is final for that operation.
    answer(
        &m.runtime,
        &m.owner.id,
        &asked,
        PermissionAnswer::Deny,
        &mut tx,
    );
    assert!(matches!(
        m.runtime.target_ticket_create(
            &m.owner.id,
            create_request(&m, &ticket, "op-1", "Refused"),
            Some(&worker()),
            &mut tx
        ),
        Err(OrchestrationError::Denied(DenialReason::PermissionDenied))
    ));
    assert_eq!(m.target_store.list_tickets().unwrap().len(), 1);
    // Reading and linking never ask; writing still does.
    let seen = read(&m, &existing.slug, &mut tx).unwrap();
    assert_eq!(seen["title"], "Already in the target");
    assert_eq!(seen["native_id"], existing.id.to_string());
    assert!(matches!(
        update(
            &m,
            &existing.id.to_string(),
            json!({"title":"Nope"}),
            &mut tx
        ),
        Err(OrchestrationError::PermissionRequired { .. })
    ));
    assert_eq!(
        m.target_store.read_ticket(&existing.id).unwrap().title,
        "Already in the target"
    );
    let linked = m
        .runtime
        .target_ticket_link(
            &m.owner.id,
            LinkTargetTicket {
                ticket: OrchestrationTicket {
                    connection_id: m.owner_connection(),
                    native_id: ticket.id.to_string(),
                },
                // The slug is a label; the stored link holds the canonical native id.
                target: target(&existing.slug),
                relation: None,
            },
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        linked,
        vec![OrchestrationLink {
            target_id: id(1),
            connection_id: m.target_connection(),
            native_id: Some(existing.id.to_string()),
            relation: Relation::Observed,
            operation_id: None,
        }]
    );
    // An observed ticket was not created by orchestration, so it has no backlink.
    let views = m
        .runtime
        .target_ticket_links(
            &m.owner.id,
            ListLinks {
                ticket: OrchestrationTicket {
                    connection_id: m.owner_connection(),
                    native_id: ticket.id.to_string(),
                },
                resolve: true,
            },
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(views[0].backlink, Some(false));
    let link_request = |native: &str, relation: Option<Relation>| LinkTargetTicket {
        ticket: OrchestrationTicket {
            connection_id: m.owner_connection(),
            native_id: ticket.id.to_string(),
        },
        target: target(native),
        relation,
    };
    let validated = m
        .runtime
        .target_ticket_link(
            &m.owner.id,
            link_request(&existing.id.to_string(), Some(Relation::Validated)),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(validated.len(), 1);
    assert_eq!(validated[0].relation, Relation::Validated);
    for relation in [Relation::Planned, Relation::Created] {
        assert!(matches!(
            m.runtime.target_ticket_link(
                &m.owner.id,
                link_request(&existing.id.to_string(), Some(relation)),
                Some(&worker()),
                &mut tx
            ),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    // A ticket that does not exist is never linked.
    let missing = id(4242);
    assert_eq!(
        op(m.runtime.target_ticket_link(
            &m.owner.id,
            link_request(&missing, None),
            Some(&worker()),
            &mut tx
        )),
        TargetOpError::NotFound
    );
    assert_eq!(m.links(&ticket).len(), 1);
    assert_eq!(op(read(&m, &missing, &mut tx)), TargetOpError::NotFound);
    assert_eq!(
        m.last_audit(),
        (
            "tickets_read".into(),
            AuditOutcome::Failed,
            Some("target_ticket_not_found".into())
        )
    );
    // A source the target does not have is refused before anything is contacted.
    assert_eq!(
        op(m.runtime.target_ticket_read(
            &m.owner.id,
            ReadTargetTicket {
                target: TargetTicket {
                    target_id: id(1),
                    connection_id: "not-a-source".into(),
                    native_id: existing.id.to_string(),
                },
            },
            Some(&worker()),
            &mut tx
        )),
        TargetOpError::NotFound
    );
    // With a remembered rule, an update goes through the target's own provider.
    allow_on(
        &m.runtime,
        &m.owner.id,
        &id(1),
        &[TargetAction::TicketsWrite],
        &[],
        &mut tx,
    );
    let updated = update(
        &m,
        &existing.id.to_string(),
        json!({"title":"Renamed by orchestration","priority":"high","tags":["a","b"],"up_next":true}),
        &mut tx,
    )
    .unwrap();
    assert_eq!(updated["title"], "Renamed by orchestration");
    let stored = m.target_store.read_ticket(&existing.id).unwrap();
    assert_eq!(stored.title, "Renamed by orchestration");
    assert_eq!(stored.tags, ["a", "b"]);
    assert!(stored.up_next);
    assert_eq!(
        m.last_audit(),
        ("tickets_write".into(), AuditOutcome::Succeeded, None)
    );
    assert!(matches!(
        update(&m, &existing.id.to_string(), json!({}), &mut tx),
        Err(OrchestrationError::Invalid(_))
    ));
    assert_eq!(
        op(update(&m, &missing, json!({"title":"x"}), &mut tx)),
        TargetOpError::NotFound
    );
    // Unlinking needs no target: it works after the target is unbound, is idempotent,
    // and never touches the target ticket.
    let binding = m.runtime.resolve(&m.reference(1), &mut tx).unwrap().binding;
    m.runtime
        .request(
            "DELETE",
            &format!(
                "/checkouts/{}/orchestration/targets/{}/binding",
                m.owner.id,
                id(1)
            ),
            &json!({"target":m.reference(1),"expected_binding_revision":binding.revision,
                "previous_destination":binding.destination}),
            &mut tx,
        )
        .unwrap();
    assert!(matches!(
        read(&m, &existing.id.to_string(), &mut tx),
        Err(OrchestrationError::Denied(DenialReason::TargetUnavailable))
    ));
    let stored_only = m
        .runtime
        .target_ticket_links(
            &m.owner.id,
            ListLinks {
                ticket: OrchestrationTicket {
                    connection_id: m.owner_connection(),
                    native_id: ticket.id.to_string(),
                },
                resolve: false,
            },
            None,
            &mut tx,
        )
        .unwrap();
    assert_eq!(stored_only[0].state, "stored");
    let unresolved = m
        .runtime
        .target_ticket_links(
            &m.owner.id,
            ListLinks {
                ticket: OrchestrationTicket {
                    connection_id: m.owner_connection(),
                    native_id: ticket.id.to_string(),
                },
                resolve: true,
            },
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    // The link stays intact with its unresolved reason.
    assert_eq!(unresolved[0].state, "unresolved");
    assert_eq!(unresolved[0].reason.as_deref(), Some("target_unavailable"));
    let unlink = || UnlinkTargetTicket {
        ticket: OrchestrationTicket {
            connection_id: m.owner_connection(),
            native_id: ticket.id.to_string(),
        },
        target_id: id(1),
        connection_id: m.target_connection(),
        native_id: Some(existing.id.to_string()),
        operation_id: None,
    };
    assert!(
        m.runtime
            .target_ticket_unlink(&m.owner.id, unlink(), &mut tx)
            .unwrap()
            .is_empty()
    );
    assert!(
        m.runtime
            .target_ticket_unlink(&m.owner.id, unlink(), &mut tx)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        m.target_store.read_ticket(&existing.id).unwrap().title,
        "Renamed by orchestration"
    );
    assert!(matches!(
        m.runtime.target_ticket_unlink(
            &m.owner.id,
            UnlinkTargetTicket {
                native_id: None,
                ..unlink()
            },
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
}

#[test]
fn hs2_az15ga_an_external_source_stays_authoritative_and_is_never_rerouted() {
    let m = machine(10, true, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsRead, TargetAction::TicketsWrite],
        &mut tx,
    );
    let ticket = m.orchestration_ticket("Coordinate");
    let external = |operation: &str| -> CreateTargetTicket {
        serde_json::from_value(json!({
            "ticket": {"connection_id": m.owner_connection(), "native_id": ticket.id.to_string()},
            "target_id": id(1), "connection_id": "external-source",
            "operation_id": operation, "title": "For the external source",
        }))
        .unwrap()
    };
    // The provider refuses: the error is reported with its code and the git source
    // of the same target is not used instead.
    assert_eq!(
        op(m.runtime
            .target_ticket_create(&m.owner.id, external("op-1"), Some(&worker()), &mut tx)),
        TargetOpError::Unsupported
    );
    assert!(m.external.opened.load(Ordering::SeqCst));
    assert_eq!(
        m.last_audit(),
        (
            "tickets_write".into(),
            AuditOutcome::Failed,
            Some("target_operation_unsupported".into())
        )
    );
    assert!(m.target_store.list_tickets().unwrap().is_empty());
    assert!(m.links(&ticket).is_empty());
    // A disabled source is refused before any client is built.
    m.external.opened.store(false, Ordering::SeqCst);
    m.runtime
        .providers
        .set_disabled("external-source", true)
        .unwrap();
    // A disabled source makes the target unusable as a whole: fail closed.
    assert!(matches!(
        m.runtime
            .target_ticket_create(&m.owner.id, external("op-2"), Some(&worker()), &mut tx),
        Err(OrchestrationError::Denied(DenialReason::TargetUnavailable))
    ));
    assert_eq!(
        op(m.runtime
            .hosted_ticket(&m.target.id, "external-source", "17")),
        TargetOpError::Disabled
    );
    assert!(!m.external.opened.load(Ordering::SeqCst));
    // Provider failures map to content-free codes.
    for (error, expected) in [
        (
            (|c: &str| ProviderError::Authentication {
                connection_id: c.into(),
                message: "synthetic secret detail".into(),
            }) as fn(&str) -> ProviderError,
            TargetOpError::Unauthorized,
        ),
        (
            |c: &str| ProviderError::RateLimited {
                connection_id: c.into(),
                retry_after_seconds: Some(30),
            },
            TargetOpError::RateLimited,
        ),
    ] {
        let other = machine(11, true, error);
        let failure = other
            .runtime
            .hosted_ticket(&other.target.id, "external-source", "17")
            .unwrap_err();
        assert!(!failure.to_string().contains("synthetic secret detail"));
        assert_eq!(op(Err::<(), _>(failure)), expected);
    }
    // An orchestration ticket in a source that cannot store links is refused clearly.
    let provider_only = fixture();
    let mut tx2 = Transaction {
        clock: &Clock,
        rng: &mut Rng(500),
    };
    assert_eq!(
        op(provider_only.runtime.target_ticket_links(
            &provider_only.owner.id,
            ListLinks {
                ticket: OrchestrationTicket {
                    connection_id: "synthetic-source".into(),
                    native_id: "17".into(),
                },
                resolve: false,
            },
            None,
            &mut tx2
        )),
        TargetOpError::LinksUnsupported
    );
}

/// A remote peer whose hosted half is a second runtime on another "machine".
pub(super) struct RemotePeer {
    pub(super) hosted: Machine,
    pub(super) calls: Mutex<Vec<(String, String)>>,
    pub(super) status_override: Mutex<Option<u16>>,
}
impl RemoteTransport for RemotePeer {
    fn probe(&self, _: &str, _: &RemoteCredentials) -> Result<RemoteSnapshot, RemoteFailure> {
        Ok(RemoteSnapshot {
            identity: RemoteIdentity {
                generation: "hs2".into(),
                protocol: 1,
                server_id: id(99),
                host_name: None,
            },
            checkouts: vec![RemoteCheckout {
                id: self.hosted.target.id.clone(),
                name: "Synthetic remote".into(),
                sources: Vec::new(),
                state: RemoteCheckoutState::Available,
                resolution_metadata_version: Some(1),
            }],
        })
    }
}
impl RemoteCheckoutTransport for RemotePeer {
    fn checkout_projection(
        &self,
        _: &str,
        _: &RemoteCredentials,
        checkout_id: &str,
    ) -> Result<CheckoutProjection, TargetFailure> {
        self.hosted
            .runtime
            .checkout_projection(checkout_id)
            .map_err(|_| TargetFailure::InvalidResponse)
    }
    fn target_call(
        &self,
        _: &str,
        _: &RemoteCredentials,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<TargetReply, TargetFailure> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((method.into(), path.into()));
        if let Some(status) = *self
            .status_override
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            return Ok(TargetReply {
                status,
                body: b"<html>not json</html>".to_vec(),
            });
        }
        let body = body
            .map(|bytes| serde_json::from_slice(bytes).unwrap())
            .unwrap_or(Value::Null);
        // Exactly what the peer's HTTP adapter does with the shared runtime.
        Ok(match self.hosted.runtime.request_now(method, path, &body) {
            Ok(value) => TargetReply {
                status: 200,
                body: serde_json::to_vec(&value).unwrap(),
            },
            Err(error) => TargetReply {
                status: error_status(&error),
                body: json!({"error":error.to_string(),"code":error.code()})
                    .to_string()
                    .into_bytes(),
            },
        })
    }
}

#[test]
fn hs2_az15ga_remote_destination_uses_the_reviewed_transport_and_peer_codes() {
    // The peer machine hosts the target; the controller machine owns the orchestration.
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
    let owner_store = FsStore::init_git(&store_path, &StoreMetadata::new("OWN")).unwrap();
    let checkouts = CheckoutRegistry::new(home.join("checkouts.json"));
    let owner = checkouts
        .register(&project, None, None, vec![store_path])
        .unwrap();
    let secrets = Arc::new(Memory::default());
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
    allow_on(
        &runtime,
        &owner.id,
        &id(1),
        &[TargetAction::TicketsWrite],
        &[],
        &mut tx,
    );
    let ticket = ops::create(
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
    let connection = peer.hosted.target_connection();
    let request = |operation: &str| -> CreateTargetTicket {
        serde_json::from_value(json!({
            "ticket": {"native_id": ticket.slug},
            "target_id": id(1), "connection_id": connection,
            "operation_id": operation, "title": "Remote target work",
        }))
        .unwrap()
    };
    let created = runtime
        .target_ticket_create(&owner.id, request("op-1"), Some(&worker()), &mut tx)
        .unwrap();
    assert!(created.created);
    assert!(created.link.recorded);
    // The ticket exists only in the peer's store; the controller holds just the link.
    let remote_tickets = peer.hosted.target_store.list_tickets().unwrap();
    assert_eq!(remote_tickets.len(), 1);
    assert_eq!(remote_tickets[0].title, "Remote target work");
    assert_eq!(
        remote_tickets[0].created_by.as_ref().unwrap().id.as_deref(),
        Some("worker-1")
    );
    assert_eq!(owner_store.list_tickets().unwrap().len(), 1);
    let calls = peer
        .calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(
        calls,
        [(
            "POST".to_owned(),
            format!("/checkouts/{}/target-tickets", peer.hosted.target.id)
        )]
    );
    let retried = runtime
        .target_ticket_create(&owner.id, request("op-1"), Some(&worker()), &mut tx)
        .unwrap();
    assert!(!retried.created);
    assert_eq!(peer.hosted.target_store.list_tickets().unwrap().len(), 1);
    let native_id = created.ticket["native_id"].as_str().unwrap().to_owned();
    let read = |runtime: &OrchestrationRuntime, native: &str, tx: &mut Transaction<'_>| {
        runtime.target_ticket_read(
            &owner.id,
            ReadTargetTicket {
                target: TargetTicket {
                    target_id: id(1),
                    connection_id: connection.clone(),
                    native_id: native.into(),
                },
            },
            Some(&worker()),
            tx,
        )
    };
    assert_eq!(
        read(&runtime, &native_id, &mut tx).unwrap()["title"],
        "Remote target work"
    );
    // The query carried percent-encoded values to the hosted route.
    let last = peer
        .calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .last()
        .cloned()
        .unwrap();
    assert_eq!(last.0, "GET");
    assert!(last.1.ends_with(&format!(
        "/target-tickets?connection_id={connection}&native_id={native_id}"
    )));
    // The peer's own refusal code is used; its message is not forwarded.
    assert_eq!(
        op(read(&runtime, &id(4242), &mut tx)),
        TargetOpError::NotFound
    );
    let updated = runtime
        .target_ticket_update(
            &owner.id,
            serde_json::from_value(json!({"target_id":id(1),"connection_id":connection,
                "native_id":native_id,"status":"started"}))
            .unwrap(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(updated["status"], "started");
    // Replies without a code are classified by status alone.
    for (status, expected) in [
        (404, TargetOpError::PeerUnsupported),
        (405, TargetOpError::PeerUnsupported),
        (403, TargetOpError::Unauthorized),
        (409, TargetOpError::Conflict),
        (429, TargetOpError::RateLimited),
        (500, TargetOpError::Failed),
    ] {
        *peer
            .status_override
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(status);
        assert_eq!(
            op(read(&runtime, &native_id, &mut tx)),
            expected,
            "{status}"
        );
    }
    *peer
        .status_override
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    // Lost credentials: the gate fails closed before the peer is called.
    let before = peer
        .calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .len();
    secrets
        .values
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    assert!(matches!(
        read(&runtime, &native_id, &mut tx),
        Err(OrchestrationError::Denied(DenialReason::TargetUnavailable))
    ));
    assert_eq!(
        peer.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len(),
        before
    );
    assert!(home.join("orchestration-audit").is_dir());
}

#[test]
fn hs2_az15ga_hosted_routes_validate_requests_and_are_not_machine_fenced() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let hosted = format!("/checkouts/{}/target-tickets", m.target.id);
    let connection = m.target_connection();
    let created = m
        .runtime
        .request(
            "POST",
            &hosted,
            &json!({"connection_id":connection,"operation_id":"op-1",
                "source":{"connection_id":"owner-source","native_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"},
                "title":"Hosted create","actor":{"role":"ai","id":"worker-9"}}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(created["created"], true);
    let native_id = created["ticket"]["native_id"].as_str().unwrap().to_owned();
    let by_operation = m
        .runtime
        .request(
            "GET",
            &format!("{hosted}?connection_id={connection}&operation_id=op-1"),
            &Value::Null,
            &mut tx,
        )
        .unwrap();
    assert_eq!(by_operation["ticket"]["native_id"], native_id);
    assert_eq!(
        m.runtime
            .request(
                "GET",
                &format!("{hosted}?connection_id={connection}&operation_id=op-none"),
                &Value::Null,
                &mut tx,
            )
            .unwrap(),
        Value::Null
    );
    let patched = m
        .runtime
        .request(
            "PATCH",
            &hosted,
            &json!({"connection_id":connection,"native_id":native_id,"details":"Changed"}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(patched["ticket"]["details"], "Changed");
    for (method, path, body) in [
        ("GET", hosted.clone(), Value::Null),
        (
            "GET",
            format!("{hosted}?connection_id={connection}"),
            Value::Null,
        ),
        (
            "GET",
            format!("{hosted}?connection_id={connection}&native_id=a&operation_id=b"),
            Value::Null,
        ),
        (
            "GET",
            format!("{hosted}?connection_id={connection}&native_id=%ZZ"),
            Value::Null,
        ),
        (
            "POST",
            hosted.clone(),
            json!({"connection_id":connection,"title":"No operation"}),
        ),
        (
            "POST",
            hosted.clone(),
            json!({"connection_id":connection,"operation_id":"op-2",
                "source":{"connection_id":"owner-source","native_id":"x"}}),
        ),
        (
            "POST",
            hosted.clone(),
            json!({"connection_id":connection,"operation_id":"op-2","title":"T","surprise":true,
                "source":{"connection_id":"owner-source","native_id":"x"}}),
        ),
        (
            "PATCH",
            hosted.clone(),
            json!({"connection_id":connection,"native_id":native_id}),
        ),
        (
            "PATCH",
            hosted.clone(),
            json!({"connection_id":connection,"native_id":"a/b","title":"x"}),
        ),
    ] {
        assert!(
            matches!(
                m.runtime.request(method, &path, &body, &mut tx),
                Err(OrchestrationError::Invalid(_))
            ),
            "{method} {path} {body}"
        );
    }
    assert_eq!(m.target_store.list_tickets().unwrap().len(), 1);
    // A remote orchestrator must reach the hosted half; the orchestrating half uses
    // this machine's grants and stays local.
    assert!(is_path(&hosted));
    assert!(!is_machine_path(&hosted));
    assert!(!is_machine_path(&format!("{hosted}?connection_id=x")));
    for operation in ["create", "read", "update", "link", "unlink", "links"] {
        let path = format!(
            "/checkouts/{}/orchestration/tickets/{operation}",
            m.owner.id
        );
        assert!(is_path(&path));
        assert!(is_machine_path(&path));
    }
    assert!(matches!(
        m.runtime.request(
            "POST",
            &format!("/checkouts/{}/orchestration/tickets/delete", m.owner.id),
            &json!({}),
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    assert!(m.home.join("checkouts.json").is_file());
}
