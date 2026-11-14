//! HS2-K2YE6A: a real second server hosting a target watches a CI gate only for a
//! repository its own checkout names (HS2-WDQZ37). The controller and the peer are
//! two servers with their own homes, joined over authenticated TLS. Synthetic data.
use super::e2e::{Fleet, Side, TARGET_B};
use super::*;
use hotsheet_jobs::JobStore;
use hotsheet_ticketing::orchestration::{AuditJournal, AuditOutcome};

fn audit_reasons(home: &std::path::Path, checkout: &str) -> Vec<(String, AuditOutcome, String)> {
    AuditJournal::new(home, checkout)
        .unwrap()
        .read(0, 200)
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| {
            (
                entry.record.action,
                entry.record.outcome,
                entry.record.reason.unwrap_or_default(),
            )
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hs2_k2ye6a_a_host_refuses_to_watch_a_repository_its_checkout_does_not_name() {
    let fleet = Fleet::new().await;
    let status = hotsheet_ticketing::git::command_in(&fleet.b_project)
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let orchestration = fleet.orchestration_ticket("Gate the remote work");
    let ticket = fleet.target_ticket(Side::Peer, "Remote work");
    let run = fleet
        .ok(
            "POST",
            "/runs",
            json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration.slug},
                "target_id":TARGET_B,"connection_id":fleet.connection(Side::Peer),
                "native_id":ticket.slug,"operation_id":"gated-run","mode":"dispatcher"}),
        )
        .await;
    let gates = format!("/runs/{}/gates", run["run_id"].as_str().unwrap());
    let peer_jobs = || {
        JobStore::at(&fleet.peer_home)
            .list(&fleet.b_checkout.id)
            .unwrap()
            .jobs
    };
    let own_jobs = || {
        JobStore::at(&fleet.home)
            .list(&fleet.owner.id)
            .unwrap()
            .jobs
    };

    // A repository the peer's checkout does not name: the peer refuses with the gate's
    // own code and starts nothing, so its credentials never read that repository.
    let (status, refused) = fleet
        .call(
            "POST",
            &gates,
            json!({"commit":"c".repeat(40),"repository":"intruder/elsewhere","actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "gate_repository_mismatch");
    assert!(peer_jobs().is_empty());
    assert!(
        own_jobs().is_empty(),
        "nor does the controller watch it itself"
    );
    // Both machines keep the refusal in their audit journals, by code.
    assert!(
        audit_reasons(&fleet.peer_home, &fleet.b_checkout.id).contains(&(
            "hosted_gate_watch".into(),
            AuditOutcome::Denied,
            "gate_repository_mismatch".into()
        )),
        "{:?}",
        audit_reasons(&fleet.peer_home, &fleet.b_checkout.id)
    );
    assert!(
        audit_reasons(&fleet.home, &fleet.owner.id)
            .iter()
            .any(|(_, _, reason)| reason == "gate_repository_mismatch"),
        "{:?}",
        audit_reasons(&fleet.home, &fleet.owner.id)
    );

    // The repository the checkout names (any spelling of it) is watched on the peer.
    let linked = fleet
        .ok(
            "POST",
            &gates,
            json!({"commit":"d".repeat(40),"repository":"Acme/Widgets","actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(linked["gate"]["state"], "pending", "{linked}");
    let jobs = peer_jobs();
    assert_eq!(jobs.len(), 1);
    assert_eq!(linked["gate"]["watch_ref"], jobs[0].id.as_str());
    assert!(own_jobs().is_empty());
}
