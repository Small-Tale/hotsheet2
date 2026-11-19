//! HS2-SHH7WW: sites of one project through the shared runtime: the HTTP-shaped
//! routes the CLI, server and MCP share, and one active run per ticket of a project
//! against a real git ticket store.
use hotsheet_model::Ticket;
use hotsheet_ticketing::orchestration::{RunError, RunState, SiteError, TargetAction};
use hotsheet_ticketing::{NewTicket, ops};
use serde_json::{Value, json};

use super::tickets::{Machine, NOW, bind_and_allow, machine, unsupported, worker};
use super::*;

fn target_ticket(m: &Machine, title: &str) -> Ticket {
    ops::create(
        &m.target_store,
        ulid::Ulid::new(),
        "TGT",
        NOW.into(),
        NewTicket {
            title: title.into(),
            ..Default::default()
        },
    )
    .unwrap()
}

fn group(m: &Machine, site: u128, project: Option<u128>, tx: &mut Transaction<'_>) -> Value {
    let revision = m.runtime.declarations(&m.owner.id).unwrap().revision;
    let body = match project {
        Some(project) => {
            json!({"expected_revision":revision,"label":"Target","site_of":id(project)})
        }
        None => json!({"expected_revision":revision,"label":"Target","clear_site_of":true}),
    };
    m.runtime
        .request(
            "PATCH",
            &format!(
                "/checkouts/{}/orchestration/targets/{}",
                m.owner.id,
                id(site)
            ),
            &body,
            tx,
        )
        .unwrap()
}

#[test]
fn hs2_shh7ww_sites_routes_add_group_list_and_refuse_with_codes() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let prefix = format!("/checkouts/{}/orchestration/targets", m.owner.id);
    let sites_path = format!("/checkouts/{}/orchestration/sites", m.owner.id);
    assert!(is_path(&sites_path) && is_machine_path(&sites_path));
    let empty = m
        .runtime
        .request("GET", &sites_path, &Value::Null, &mut tx)
        .unwrap();
    assert_eq!(empty, json!({"revision":null,"projects":[]}));
    let first = m
        .runtime
        .request(
            "POST",
            &prefix,
            &json!({"expected_revision":null,"target_id":id(1),"label":"Widgets"}),
            &mut tx,
        )
        .unwrap();
    // A site of a project that is not declared: 404 with its own code, nothing written.
    let unknown = m
        .runtime
        .request(
            "POST",
            &prefix,
            &json!({"expected_revision":first["revision"],"target_id":id(2),"label":"Remote","site_of":id(9)}),
            &mut tx,
        )
        .unwrap_err();
    assert_eq!(unknown.code(), "site_project_unknown");
    assert_eq!(error_status(&unknown), 404);
    let added = m
        .runtime
        .request(
            "POST",
            &prefix,
            &json!({"expected_revision":first["revision"],"target_id":id(2),"label":"Widgets remote","site_of":id(1)}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(added["targets"][1]["site_of"], id(1));
    // A chain is a conflict.
    let chain = m
        .runtime
        .request(
            "POST",
            &prefix,
            &json!({"expected_revision":added["revision"],"target_id":id(3),"label":"Chain","site_of":id(2)}),
            &mut tx,
        )
        .unwrap_err();
    assert!(matches!(chain, OrchestrationError::Site(SiteError::Chain)));
    assert_eq!(error_status(&chain), 409);
    // Setting and clearing at once is a bad request.
    assert!(matches!(
        m.runtime.request(
            "PATCH",
            &format!("{prefix}/{}", id(2)),
            &json!({"expected_revision":added["revision"],"label":"Widgets remote","site_of":id(1),"clear_site_of":true}),
            &mut tx,
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    let sites = m
        .runtime
        .request("GET", &sites_path, &Value::Null, &mut tx)
        .unwrap();
    assert_eq!(sites["revision"], added["revision"]);
    assert_eq!(sites["projects"].as_array().unwrap().len(), 1);
    let project = &sites["projects"][0];
    assert_eq!(project["project_id"], id(1));
    assert_eq!(project["sites"][0]["target_id"], id(1));
    assert_eq!(project["sites"][0]["project"], true);
    assert_eq!(project["sites"][1]["target_id"], id(2));
    assert_eq!(project["sites"][1]["project"], false);
    assert_eq!(project["sites"][1]["destination"], Value::Null);
    // Removing the project while its site is active is refused.
    let removing = m
        .runtime
        .request(
            "DELETE",
            &format!("{prefix}/{}", id(1)),
            &json!({"expected_revision":added["revision"]}),
            &mut tx,
        )
        .unwrap_err();
    assert_eq!(removing.code(), "site_project_has_sites");
    // A request field an older host does not know is refused there; this host knows it.
    assert!(
        m.runtime
            .request(
                "POST",
                &prefix,
                &json!({"expected_revision":added["revision"],"label":"X","site_of":id(1),"site_priority":1}),
                &mut tx,
            )
            .is_err()
    );
}

fn dispatch_through(
    m: &Machine,
    orchestration: &Ticket,
    site: u128,
    target: &Ticket,
    operation: &str,
    worker_id: &str,
    tx: &mut Transaction<'_>,
) -> Result<RunView, OrchestrationError> {
    m.runtime.run_dispatch(
        &m.owner.id,
        serde_json::from_value(json!({
            "ticket": {"connection_id": m.owner_connection(), "native_id": orchestration.id.to_string()},
            "target_id": id(site), "connection_id": m.target_connection(),
            "native_id": target.id.to_string(), "operation_id": operation,
            "worker_id": worker_id, "worker_label": worker_id,
        }))
        .unwrap(),
        Some(&worker()),
        tx,
    )
}

/// Two targets bound to the same checkout: grouped, one ticket has one active run
/// whichever site dispatches it, and nothing is asked of the target the second time.
#[test]
fn hs2_shh7ww_a_ticket_dispatched_through_two_sites_has_one_active_run() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let actions = [TargetAction::TicketsRead, TargetAction::WorkStart];
    bind_and_allow(&m, 1, &actions, &mut tx);
    bind_and_allow(&m, 2, &actions, &mut tx);
    group(&m, 2, Some(1), &mut tx);
    let sites = m.runtime.sites(&m.owner.id).unwrap();
    assert_eq!(sites.projects.len(), 1);
    assert_eq!(sites.projects[0].sites.len(), 2);
    assert!(
        sites.projects[0]
            .sites
            .iter()
            .all(|site| site.destination.is_some())
    );

    let orchestration = m.orchestration_ticket("Coordinate");
    let first = target_ticket(&m, "First target ticket");
    let run = dispatch_through(&m, &orchestration, 1, &first, "op-1", "worker-a", &mut tx)
        .unwrap()
        .run;
    assert_eq!(run.state, RunState::Dispatched);
    let second = dispatch_through(&m, &orchestration, 2, &first, "op-2", "worker-b", &mut tx);
    assert!(matches!(
        second,
        Err(OrchestrationError::Run(RunError::TargetActive))
    ));
    assert_eq!(m.runtime.runs().list(&m.owner.id, false).unwrap().len(), 1);
    // The target's claim is untouched by the refused second dispatch.
    let held = m.target_store.read_ticket(&first.id).unwrap();
    assert_eq!(held.claimed_by.as_deref(), Some("worker-a"));
    assert_eq!(held.claim_count, 1);

    // Separating the sites while a run stands on a ticket is fine: one run per site.
    group(&m, 2, None, &mut tx);
    // Grouping again while only one run stands is fine too; a second run through the
    // other site after separation would make the grouping refused (HS2-SHH7WW).
    group(&m, 2, Some(1), &mut tx);
}

/// HS2-D4G81Q: the sites view reads what each bound site's machine is only when asked,
/// through the gated capacity read with its actor in the query or a body; a site this
/// machine does not bind is not read, and a refused read names its code.
#[test]
fn hs2_d4g81q_sites_view_reads_each_bound_sites_machine_when_asked() {
    use hotsheet_ticketing::orchestration::{HostCapabilities, OsFamily};
    use std::sync::atomic::Ordering;
    let mut m = machine(10, false, unsupported);
    let probe = super::capacity::probed(&mut m);
    probe.set(|observation| {
        observation.capabilities = Some(HostCapabilities::bare(OsFamily::Macos, "arm64"));
    });
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[TargetAction::GatesObserve], &mut tx);
    // A second site of the project, declared but not bound on this machine.
    let revision = m.runtime.declarations(&m.owner.id).unwrap().revision;
    m.runtime
        .request(
            "POST",
            &format!("/checkouts/{}/orchestration/targets", m.owner.id),
            &json!({"expected_revision":revision,"target_id":id(2),"label":"Widgets laptop","site_of":id(1)}),
            &mut tx,
        )
        .unwrap();
    let path = format!("/checkouts/{}/orchestration/sites", m.owner.id);

    // Unasked, the view contacts nothing and carries no machine.
    let plain = m
        .runtime
        .request("GET", &path, &Value::Null, &mut tx)
        .unwrap();
    assert_eq!(probe.asked.load(Ordering::SeqCst), 0);
    for site in plain["projects"][0]["sites"].as_array().unwrap() {
        assert!(site.get("capabilities").is_none() && site.get("capabilities_error").is_none());
    }

    // Asked with the actor in the query: the bound site's machine, and nothing for the other.
    let read = m
        .runtime
        .request(
            "GET",
            &format!("{path}?capabilities=true&actor_id=worker-1&actor_role=ai"),
            &Value::Null,
            &mut tx,
        )
        .unwrap();
    assert_eq!(probe.asked.load(Ordering::SeqCst), 1);
    let sites = &read["projects"][0]["sites"];
    assert_eq!(sites[0]["target_id"], id(1));
    assert_eq!(sites[0]["capabilities"]["os"], "macos");
    assert_eq!(sites[0]["capabilities"]["arch"], "arm64");
    assert!(sites[0].get("capabilities_error").is_none());
    assert_eq!(sites[1]["target_id"], id(2));
    assert!(sites[1].get("capabilities").is_none() && sites[1].get("capabilities_error").is_none());
    // The same read with the actor in a body, as the CLI and MCP send it.
    let posted = m
        .runtime
        .request(
            "POST",
            &path,
            &json!({"capabilities":true,"actor":{"role":"ai","id":"worker-1"}}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(posted, read);
    assert_eq!(
        m.runtime
            .request("POST", &path, &json!({"capabilities":false}), &mut tx)
            .unwrap(),
        plain
    );
    // A machine that reports nothing about itself reads as no capabilities, not an error.
    probe.set(|observation| observation.capabilities = None);
    let bare = m
        .runtime
        .request(
            "GET",
            &format!("{path}?capabilities=true&actor_id=worker-1&actor_role=ai"),
            &Value::Null,
            &mut tx,
        )
        .unwrap();
    assert!(
        bare["projects"][0]["sites"][0]
            .get("capabilities")
            .is_none()
    );
    assert!(
        bare["projects"][0]["sites"][0]
            .get("capabilities_error")
            .is_none()
    );

    // A read the gate refuses names its code and hides nothing else.
    let refused = m
        .runtime
        .request(
            "GET",
            &format!("{path}?capabilities=true"),
            &Value::Null,
            &mut tx,
        )
        .unwrap();
    let site = &refused["projects"][0]["sites"][0];
    assert!(site.get("capabilities").is_none());
    assert!(
        site["capabilities_error"]
            .as_str()
            .is_some_and(|code| !code.is_empty())
    );
    assert_eq!(refused["projects"][0]["sites"][1]["target_id"], id(2));
    // Malformed requests are refused as a whole.
    for (method, wrong, body) in [
        ("GET", format!("{path}?capabilities=yes"), Value::Null),
        ("POST", path.clone(), json!({"capabilities":true,"extra":1})),
    ] {
        assert!(matches!(
            m.runtime.request(method, &wrong, &body, &mut tx),
            Err(OrchestrationError::Invalid(_))
        ));
    }
}
