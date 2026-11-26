//! HS2-D85G19: listing and searching a target's open tickets through the gate, for a
//! local git source, an external source, a remote peer and an older peer without the
//! route. Synthetic stores and identities only.
use hotsheet_model::Status;
use hotsheet_ticketing::orchestration::{AuditOutcome, DenialReason, TargetOpError};
use hotsheet_ticketing::remote_servers::RegisterRemote;
use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, ops};
use serde_json::{Value, json};

use super::tickets::{
    Machine, NOW, RemotePeer, bind_and_allow, machine, op, unbind, unsupported, worker,
};
use super::*;

fn seed(store: &FsStore, title: &str, status: Status, tags: &[&str], category: &str) {
    ops::create(
        store,
        ulid::Ulid::new(),
        "TGT",
        NOW.into(),
        NewTicket {
            title: title.into(),
            status,
            category: category.into(),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            ..Default::default()
        },
    )
    .unwrap();
}

fn search(m: &Machine, extra: Value) -> ListTargetTickets {
    let mut body = json!({"target_id": id(1), "connection_id": m.target_connection()});
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(body).unwrap()
}

fn titles(page: &TargetTicketPage) -> Vec<String> {
    let mut titles: Vec<_> = page.tickets.iter().map(|row| row.title.clone()).collect();
    titles.sort();
    titles
}

#[test]
fn hs2_d85g19_lists_and_searches_open_target_tickets_through_the_gate() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    // Empty first: a bound target with no ticket lists nothing and has no next page.
    bind_and_allow(&m, 1, &[], &mut tx);
    let empty = m
        .runtime
        .target_ticket_list(&m.owner.id, search(&m, json!({})), Some(&worker()), &mut tx)
        .unwrap();
    assert!(empty.tickets.is_empty());
    assert_eq!(empty.next_cursor, None);
    // Reading needs no remembered rule: adding the project consented to it.
    assert_eq!(
        m.last_audit(),
        ("tickets_read".into(), AuditOutcome::Succeeded, None)
    );
    seed(
        &m.target_store,
        "Add the login page",
        Status::NotStarted,
        &["web"],
        "feature",
    );
    seed(
        &m.target_store,
        "Fix the login redirect",
        Status::Started,
        &["web"],
        "bug",
    );
    seed(
        &m.target_store,
        "Document the API",
        Status::NotStarted,
        &["docs"],
        "task",
    );
    seed(
        &m.target_store,
        "Old login work",
        Status::Completed,
        &["web"],
        "feature",
    );
    let list = |extra: Value, tx: &mut Transaction<'_>| {
        m.runtime
            .target_ticket_list(&m.owner.id, search(&m, extra), Some(&worker()), tx)
    };
    // Refill: only open tickets are listed, with labels and identifiers only.
    let all = list(json!({}), &mut tx).unwrap();
    assert_eq!(
        titles(&all),
        [
            "Add the login page",
            "Document the API",
            "Fix the login redirect"
        ]
    );
    let row = all
        .tickets
        .iter()
        .find(|row| row.title == "Fix the login redirect")
        .unwrap();
    assert_eq!(row.status, Status::Started);
    assert_eq!(row.category, "bug");
    assert_eq!(row.tags, ["web"]);
    assert!(row.slug.starts_with("TGT-"));
    let wire = serde_json::to_value(row).unwrap();
    assert!(wire.get("details").is_none() && wire.get("notes").is_none());
    // Text, tag and category narrow the page; a closed match stays out.
    assert_eq!(
        titles(&list(json!({"text": "LOGIN"}), &mut tx).unwrap()),
        ["Add the login page", "Fix the login redirect"]
    );
    assert_eq!(
        titles(&list(json!({"tag": "docs"}), &mut tx).unwrap()),
        ["Document the API"]
    );
    assert_eq!(
        titles(&list(json!({"text": "login", "category": "bug"}), &mut tx).unwrap()),
        ["Fix the login redirect"]
    );
    assert!(
        list(json!({"text": "nothing like this"}), &mut tx)
            .unwrap()
            .tickets
            .is_empty()
    );
    // Pages are bounded and follow the provider's cursor to the end, with no repeat.
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..3 {
        let mut extra = json!({"limit": 2});
        if let Some(cursor) = &cursor {
            extra["cursor"] = json!(cursor);
        }
        let page = list(extra, &mut tx).unwrap();
        assert!(page.tickets.len() <= 2);
        seen.extend(page.tickets.iter().map(|row| row.native_id.clone()));
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(cursor, None);
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 3);
    // Repeating the same search answers the same page.
    assert_eq!(
        list(json!({"text": "login"}), &mut tx).unwrap(),
        list(json!({"text": "login"}), &mut tx).unwrap()
    );
    // Bounds are checked before the gate, so nothing is audited for them.
    let audited = m.runtime.audit(&m.owner.id, 0, 500).unwrap().entries.len();
    for extra in [
        json!({"limit": 0}),
        json!({"limit": 101}),
        json!({"text": ""}),
        json!({"text": "x".repeat(201)}),
        json!({"text": "line\nbreak"}),
        json!({"cursor": ""}),
        json!({"cursor": "has space"}),
        json!({"cursor": "c".repeat(513)}),
        json!({"category": "not a category"}),
        json!({"tag": " "}),
    ] {
        assert!(
            matches!(
                list(extra.clone(), &mut tx),
                Err(OrchestrationError::Invalid(_))
            ),
            "{extra}"
        );
    }
    assert_eq!(
        m.runtime.audit(&m.owner.id, 0, 500).unwrap().entries.len(),
        audited
    );
    // A cursor the provider cannot read is the caller's mistake, never a conflict.
    assert_eq!(
        op(list(json!({"cursor": "not-a-number"}), &mut tx)),
        TargetOpError::Invalid
    );
    // A source the target does not list is not found, and nothing is rerouted.
    assert_eq!(
        op(m.runtime.target_ticket_list(
            &m.owner.id,
            serde_json::from_value(json!({"target_id": id(1), "connection_id": "another-source"}))
                .unwrap(),
            Some(&worker()),
            &mut tx,
        )),
        TargetOpError::NotFound
    );
    assert_eq!(
        m.last_audit(),
        (
            "tickets_read".into(),
            AuditOutcome::Failed,
            Some("target_ticket_not_found".into())
        )
    );
    // Unknown fields are refused rather than ignored.
    assert!(
        serde_json::from_value::<ListTargetTickets>(
            json!({"target_id": id(1), "connection_id": "c", "status": "completed"})
        )
        .is_err()
    );
    // An unbound target fails closed before its store is read.
    unbind(&m, 1, &mut tx);
    assert!(matches!(
        list(json!({}), &mut tx),
        Err(OrchestrationError::Denied(DenialReason::TargetUnavailable))
    ));
    // An undeclared target is refused too.
    assert!(matches!(
        m.runtime.target_ticket_list(
            &m.owner.id,
            serde_json::from_value(
                json!({"target_id": id(7), "connection_id": m.target_connection()})
            )
            .unwrap(),
            Some(&worker()),
            &mut tx,
        ),
        Err(OrchestrationError::Denied(_))
    ));
}

#[test]
fn hs2_d85g19_an_external_source_answers_for_itself() {
    let m = machine(10, true, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[], &mut tx);
    seed(
        &m.target_store,
        "In the git source",
        Status::NotStarted,
        &[],
        "task",
    );
    // The external provider refuses; its code is reported and the git source of the
    // same target is not listed instead.
    assert_eq!(
        op(m.runtime.target_ticket_list(
            &m.owner.id,
            serde_json::from_value(json!({"target_id": id(1), "connection_id": "external-source"}))
                .unwrap(),
            Some(&worker()),
            &mut tx,
        )),
        TargetOpError::Unsupported
    );
    assert!(m.external.opened.load(Ordering::SeqCst));
}

#[test]
fn hs2_d85g19_the_hosted_route_validates_its_query() {
    let m = machine(10, false, unsupported);
    seed(
        &m.target_store,
        "Hosted row",
        Status::NotStarted,
        &["a b"],
        "task",
    );
    let base = format!("/checkouts/{}/target-tickets/list", m.target.id);
    let connection = m.target_connection();
    let page = m
        .runtime
        .request_now(
            "GET",
            &format!("{base}?connection_id={connection}&tag=a%20b&limit=5"),
            &Value::Null,
        )
        .unwrap();
    assert_eq!(page["tickets"][0]["title"], "Hosted row");
    assert!(page.get("next_cursor").is_none());
    for query in [
        String::new(),
        format!("?connection_id={connection}&limit=0"),
        format!("?connection_id={connection}&limit=abc"),
        format!("?connection_id={connection}&limit=99999999999"),
        format!("?connection_id={connection}&text=a&text=b"),
        format!("?connection_id={connection}&text=%ZZ"),
    ] {
        assert!(
            matches!(
                m.runtime
                    .request_now("GET", &format!("{base}{query}"), &Value::Null),
                Err(OrchestrationError::Invalid(_))
            ),
            "{query}"
        );
    }
    // The hosted route is ordinary authenticated project data, not machine-fenced.
    assert!(is_path(&base));
    assert!(!is_machine_path(&base));
    assert!(is_machine_path(&format!(
        "/checkouts/{}/orchestration/tickets/list",
        m.owner.id
    )));
}

#[test]
fn hs2_d85g19_a_remote_target_is_listed_over_the_reviewed_transport() {
    let peer = Arc::new(RemotePeer {
        hosted: machine(99, false, unsupported),
        calls: Mutex::new(Vec::new()),
        status_override: Mutex::new(None),
        offline: Default::default(),
    });
    seed(
        &peer.hosted.target_store,
        "Remote open work",
        Status::NotStarted,
        &[],
        "task",
    );
    seed(
        &peer.hosted.target_store,
        "Remote finished work",
        Status::Completed,
        &[],
        "task",
    );
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let project = root.path().join("owner");
    std::fs::create_dir(&project).unwrap();
    let store_path = root.path().join("owner-store");
    FsStore::init_git(&store_path, &StoreMetadata::new("OWN")).unwrap();
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
                    target: TargetRef {
                        owner_checkout_id: owner.id.clone(),
                        target_id: id(1),
                    },
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
    let connection = peer.hosted.target_connection();
    let list = |extra: Value, tx: &mut Transaction<'_>| {
        let mut body = json!({"target_id": id(1), "connection_id": connection});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        runtime.target_ticket_list(
            &owner.id,
            serde_json::from_value(body).unwrap(),
            Some(&worker()),
            tx,
        )
    };
    let page = list(json!({"text": "remote", "limit": 10}), &mut tx).unwrap();
    assert_eq!(titles(&page), ["Remote open work"]);
    let last = peer
        .calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .last()
        .cloned()
        .unwrap();
    assert_eq!(
        last,
        (
            "GET".to_owned(),
            format!(
                "/checkouts/{}/target-tickets/list?connection_id={connection}&text=remote&limit=10",
                peer.hosted.target.id
            )
        )
    );
    // An older peer without the route answers 404 with no code: reported as such.
    *peer
        .status_override
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(404);
    assert_eq!(op(list(json!({}), &mut tx)), TargetOpError::PeerUnsupported);
    *peer
        .status_override
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    assert!(list(json!({}), &mut tx).is_ok());
}
