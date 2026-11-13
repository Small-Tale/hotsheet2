//! HS2-2EY9DP: the orchestration loop of `e2e_loop.rs` under failure: a controller
//! restart, a partial target outage, lost access, cancellation, duplicate requests and
//! out-of-order events. Each is its own test so that a failure names its seam.
use super::e2e::{Fleet, Side, TARGET_B};
use super::e2e_loop::{placed, planned, works};
use super::*;
use std::time::Duration;

const MINUTE: Duration = Duration::from_secs(60);

fn started(entry: &Value) -> bool {
    entry["kind"] == "start" && entry["outcome"] == "started"
}

async fn starts(fleet: &Fleet) -> Vec<Value> {
    fleet
        .decisions()
        .await
        .into_iter()
        .filter(started)
        .collect()
}

/// HS2-2EY9DP priority 2: the controller restarts twice during the loop, once while
/// target A's CI gate waits and once while the remote session works target B. Each
/// new process resumes from its home alone: the waiting gate is observed again, the
/// remote session is adopted rather than started twice, and the plan finishes.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_2ey9dp_a_restarted_controller_resumes_the_plan_without_duplicate_work() {
    let mut fleet = Fleet::new().await;
    let walked = planned(&mut fleet).await;
    let a = placed(&fleet, &walked.step_a, Side::Controller).await;
    works(&fleet, Side::Controller, &a, &walked.a).await;
    let commit = "b".repeat(40);
    fleet.provider.run(51, &commit, None);
    let linked = fleet
        .ok(
            "POST",
            &format!("/runs/{}/gates", a.run_id),
            json!({"commit":commit,"repository":"acme/widgets","actor":Fleet::ai()}),
        )
        .await;
    let gate = linked["gate"]["gate_id"].as_str().unwrap().to_owned();
    fleet.finish(Side::Controller, &walked.a, &a.worker).await;
    fleet
        .run_until(&a.run_id, "completed", |status| {
            status["state"] == "completed"
        })
        .await;

    // 1. Restart while the gate waits on CI. The provider turns green afterwards; the
    //    new process follows the waiting gate from its start-up alone.
    fleet.restart_controller().await;
    fleet.provider.run(51, &commit, Some("success"));
    fleet.gate_until(&gate, "passed").await;
    let b = placed(&fleet, &walked.step_b, Side::Peer).await;
    works(&fleet, Side::Peer, &b, &walked.b).await;

    // 2. Restart while the remote session works B. The new controller adopts it.
    let before = starts(&fleet).await.len();
    fleet.restart_controller().await;
    let reconciled = fleet
        .ok(
            "POST",
            &format!("/runs/{}/reconcile", b.run_id),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(
        reconciled["reconciliation"]["disposition"], "continue",
        "{reconciled}"
    );
    let session_finding = reconciled["reconciliation"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["source"] == "session")
        .cloned();
    assert_eq!(
        session_finding,
        Some(json!({"source":"session","verification":"verified"})),
        "{reconciled}"
    );
    // The restarted loop's passes start nothing: B's session still works it.
    let tick = fleet
        .ok("POST", "/scheduler/tick", json!({"actor":Fleet::ai()}))
        .await;
    assert_eq!(tick["mode"], "applied", "{tick}");
    assert_eq!(starts(&fleet).await.len(), before);
    assert_eq!(
        fleet
            .drawer(Side::Peer)
            .await
            .iter()
            .filter(|tab| tab["alive"] == true)
            .count(),
        1
    );

    // 3. B finishes; the restarted controller follows the peer's feed to completion.
    fleet.finish(Side::Peer, &walked.b, &b.worker).await;
    fleet
        .run_until(&b.run_id, "completed", |status| {
            status["state"] == "completed"
        })
        .await;
    fleet.step_until(&walked.step_b, "finished").await;
    let stop = fleet
        .decided(
            "B's wind-down",
            |entry| {
                entry["kind"] == "wind_down"
                    && entry["outcome"] == "wind_down_requested"
                    && entry["session_id"] == b.session_id.as_str()
            },
            3 * MINUTE,
        )
        .await;
    assert_eq!(stop["reason"], "idle");
    assert_eq!(fleet.runs().await.len(), 2);
    fleet.shut_down().await;
}

/// HS2-2EY9DP priority 2: the peer drops off the network while its session works B,
/// and target A's work goes on meanwhile. The run says it cannot confirm B rather than
/// guessing, the loop starts nothing in its place, and when the peer is back the run
/// catches up with what happened there.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_2ey9dp_a_partial_target_outage_holds_only_the_unreachable_target() {
    let mut fleet = Fleet::new().await;
    let orchestration = fleet.orchestration_ticket("Coordinate two independent changes");
    let a_ticket = fleet.target_ticket(Side::Controller, "Synthetic independent change A");
    let b_ticket = fleet.target_ticket(Side::Peer, "Synthetic independent change B");
    let step_a = fleet
        .plan(&orchestration, Side::Controller, &a_ticket, json!([]))
        .await["step"]["step_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let step_b = fleet
        .plan(&orchestration, Side::Peer, &b_ticket, json!([]))
        .await["step"]["step_id"]
        .as_str()
        .unwrap()
        .to_owned();
    fleet.automatic().await;
    let a = placed(&fleet, &step_a, Side::Controller).await;
    let b = placed(&fleet, &step_b, Side::Peer).await;
    works(&fleet, Side::Peer, &b, &b_ticket).await;

    // The peer goes away. B's session keeps working there and finishes its ticket.
    fleet.peer_offline().await;
    fleet.finish(Side::Peer, &b_ticket, &b.worker).await;
    let unreachable = fleet
        .run_until(&b.run_id, "marked unreachable", |status| {
            status["reachability"] == "unreachable"
        })
        .await;
    assert_eq!(unreachable["state"], "dispatched", "{unreachable}");
    assert_eq!(unreachable["stale"], true, "{unreachable}");
    // A person asking now gets the truth: B cannot be confirmed.
    let (status, synced) = fleet
        .call(
            "POST",
            &format!("/runs/{}/sync", b.run_id),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    assert_ne!(status, StatusCode::OK, "{synced}");

    // Target A is untouched by B's outage: its work goes through to completion.
    works(&fleet, Side::Controller, &a, &a_ticket).await;
    fleet.finish(Side::Controller, &a_ticket, &a.worker).await;
    fleet
        .run_until(&a.run_id, "completed", |status| {
            status["state"] == "completed"
        })
        .await;
    // The loop never started a second session for B while it could not see it.
    let tick = fleet
        .ok("POST", "/scheduler/tick", json!({"actor":Fleet::ai()}))
        .await;
    assert_eq!(tick["mode"], "applied", "{tick}");
    let log = starts(&fleet).await;
    assert_eq!(
        log.iter()
            .filter(|entry| entry["step_id"] == step_b.as_str())
            .count(),
        1,
        "{log:#?}"
    );

    // The peer is back: the follower replays what it missed and B completes.
    fleet.peer_online().await;
    let done = fleet
        .run_until(&b.run_id, "completed after the outage", |status| {
            status["state"] == "completed"
        })
        .await;
    assert_eq!(done["reachability"], "reachable", "{done}");
    assert_eq!(done["observed"]["status"], "completed", "{done}");
    fleet.step_until(&step_b, "finished").await;
    assert_eq!(fleet.runs().await.len(), 2);
    assert_eq!(fleet.sessions().await.len(), 2);
    fleet.shut_down().await;
}

/// One step on the remote target B, placed and claimed by its session.
async fn one_remote_step(
    fleet: &mut Fleet,
    title: &str,
) -> (String, hotsheet_model::Ticket, super::e2e_loop::Placed) {
    let orchestration = fleet.orchestration_ticket("Coordinate one remote change");
    let ticket = fleet.target_ticket(Side::Peer, title);
    let step = fleet
        .plan(&orchestration, Side::Peer, &ticket, json!([]))
        .await["step"]["step_id"]
        .as_str()
        .unwrap()
        .to_owned();
    fleet.automatic().await;
    let b = placed(fleet, &step, Side::Peer).await;
    works(fleet, Side::Peer, &b, &ticket).await;
    (step, ticket, b)
}

/// HS2-2EY9DP priority 3: a person revokes the scheduler's grant on B while its
/// session works there, and later a replacement grant expires. Each denial is the
/// typed code, the loop neither starts nor stops anything on B without authority, and
/// a new grant lets the run finish.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_2ey9dp_lost_access_mid_run_stops_the_orchestrator_acting_on_the_target() {
    let mut fleet = Fleet::new().await;
    let (step, ticket, b) = one_remote_step(&mut fleet, "Synthetic change under revocation").await;
    let before = starts(&fleet).await.len();

    // 1. Revoked.
    let grant = fleet.grants[TARGET_B].clone();
    fleet
        .ok(
            "DELETE",
            &format!("/targets/{TARGET_B}/grants/{grant}"),
            json!({"actor":Fleet::person()}),
        )
        .await;
    let (status, denied) = fleet
        .call(
            "POST",
            &format!("/runs/{}/sync", b.run_id),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");
    assert_eq!(denied["code"], "grant_revoked", "{denied}");
    // The loop's next pass neither starts a replacement nor stops the session it can
    // no longer read: it keeps it, busy and not safe, for a person to decide.
    let tick = fleet
        .ok("POST", "/scheduler/tick", json!({"actor":Fleet::ai()}))
        .await;
    assert_eq!(tick["mode"], "applied", "{tick}");
    assert_eq!(starts(&fleet).await.len(), before);
    let session = fleet.session(&b.session_id).await;
    assert_eq!(session["state"], "running", "{session}");
    let stopped = fleet
        .decisions()
        .await
        .into_iter()
        .filter(|entry| entry["kind"] == "wind_down")
        .count();
    assert_eq!(stopped, 0);
    // The run record is left as the target last confirmed it.
    let shown = fleet
        .ok("GET", &format!("/runs/{}", b.run_id), Value::Null)
        .await;
    assert_eq!(shown["state"], "dispatched", "{shown}");

    // 2. A replacement grant that expires before the work is done.
    let expires = super::session_work::now_ms() + 2_000;
    fleet.grant(TARGET_B, expires).await;
    let synced = fleet
        .ok(
            "POST",
            &format!("/runs/{}/sync", b.run_id),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(synced["state"], "dispatched", "{synced}");
    // Waiting for a known deadline, not polling for a state.
    tokio::time::sleep(Duration::from_millis(
        expires.saturating_sub(super::session_work::now_ms()) + 200,
    ))
    .await;
    let (status, denied) = fleet
        .call(
            "POST",
            &format!("/runs/{}/sync", b.run_id),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");
    assert_eq!(denied["code"], "grant_expired", "{denied}");
    let audit = fleet
        .ok("GET", "/audit?after=0&limit=500", Value::Null)
        .await;
    let reasons: Vec<_> = audit["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["outcome"] == "denied")
        .map(|entry| entry["reason"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert!(reasons.contains(&"grant_revoked".to_owned()), "{reasons:?}");
    assert!(reasons.contains(&"grant_expired".to_owned()), "{reasons:?}");

    // 3. The work is finished on the peer meanwhile; a new grant lets the run see it.
    fleet.finish(Side::Peer, &ticket, &b.worker).await;
    fleet
        .grant(TARGET_B, super::session_work::now_ms() + 3_600_000)
        .await;
    let synced = fleet
        .ok(
            "POST",
            &format!("/runs/{}/sync", b.run_id),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(synced["state"], "completed", "{synced}");
    fleet.step_until(&step, "finished").await;
    assert_eq!(starts(&fleet).await.len(), before);
    fleet.shut_down().await;
}

/// HS2-2EY9DP priority 3: a person cancels the run its remote session works. The
/// pooled worker's claim is not the orchestrator's to release, and because the step is
/// still planned, but a person's cancel holds it (HS2-HV9ED3): nothing is dispatched
/// again, `explain` says `cancelled_by_person` and the session is wound down at its
/// next safe point. A run dispatched again by hand ends the hold; removing the step
/// and cancelling that run withdraws its Up Next mark on the peer.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_2ey9dp_cancelling_a_remote_run_lets_go_without_touching_the_workers_claim() {
    let mut fleet = Fleet::new().await;
    let (step, ticket, b) = one_remote_step(&mut fleet, "Synthetic change to cancel").await;
    let cancelled = fleet
        .ok(
            "POST",
            &format!("/runs/{}/cancel", b.run_id),
            json!({"actor":Fleet::person()}),
        )
        .await;
    assert_eq!(cancelled["state"], "cancelled", "{cancelled}");
    assert_eq!(cancelled["cancel_requested"], false, "{cancelled}");
    let peer_ticket = || {
        FsStore::open(&fleet.b_store)
            .unwrap()
            .read_ticket(&ticket.id)
            .unwrap()
    };
    assert_eq!(peer_ticket().claimed_by.as_deref(), Some(b.worker.as_str()));

    // HS2-HV9ED3: a person's cancel holds the step. The loop dispatches nothing for it
    // and winds its session down at the next safe point, which the worker's claim
    // keeps from coming yet.
    let held = fleet
        .decided(
            "wind-down of the cancelled run's session",
            |entry| entry["kind"] == "wind_down" && entry["reason"] == "run_cancelled",
            Duration::from_secs(60),
        )
        .await;
    assert_eq!(held["session_id"], b.session_id.as_str(), "{held}");
    assert_eq!(held["outcome"], "not_at_safe_point", "{held}");
    let tick = fleet
        .ok("POST", "/scheduler/tick", json!({"actor":Fleet::ai()}))
        .await;
    assert_eq!(tick["mode"], "applied", "{tick}");
    assert_eq!(fleet.runs().await.len(), 1);
    let explained = fleet
        .ok(
            "POST",
            "/scheduler/explain",
            json!({"step_id":step,"actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(explained["reason"], "cancelled_by_person", "{explained}");
    assert_eq!(peer_ticket().claimed_by.as_deref(), Some(b.worker.as_str()));
    // The session's worker lets go, as it was told to.
    let (status, released) = super::orchestration::api(
        fleet.side_router(Side::Peer),
        "POST",
        &format!("/tickets/{}/release", ticket.id),
        json!({"worker": b.worker}),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{released}");
    assert_eq!(peer_ticket().claimed_by, None);

    // The work is dispatched again by hand, which ends the hold. Nobody holds the
    // ticket, so the new run waits queued for the target's dispatcher.
    let orchestration = fleet
        .ok("GET", &format!("/runs/{}", b.run_id), Value::Null)
        .await["ticket"]["native_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let redispatched = fleet
        .ok(
            "POST",
            "/runs",
            json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration},
                "target_id":TARGET_B,"connection_id":fleet.connection(Side::Peer),
                "native_id":ticket.slug,"operation_id":"manual-again","mode":"dispatcher"}),
        )
        .await;
    assert_eq!(redispatched["state"], "queued", "{redispatched}");
    assert!(peer_ticket().up_next);

    // A person removes the step and cancels the new run: its mark is withdrawn on the
    // peer, over the reviewed transport, and nothing is dispatched again.
    fleet
        .ok(
            "POST",
            "/readiness/remove",
            json!({"actor":Fleet::person(),"step_id":step}),
        )
        .await;
    let cancel = format!("/runs/{}/cancel", redispatched["run_id"].as_str().unwrap());
    // HS2-WC3W2G: the person holds no grant of their own, so the gate denies telling
    // the target. The answer is that denial, and the run records nothing.
    let (status, refused) = fleet
        .call("POST", &cancel, json!({"actor":Fleet::person()}))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert_eq!(refused["code"], "no_grant", "{refused}");
    let still = fleet
        .ok("GET", &cancel.replace("/cancel", ""), Value::Null)
        .await;
    assert_eq!(still["state"], "queued", "{still}");
    assert_eq!(still["cancel_requested"], false, "{still}");
    assert!(peer_ticket().up_next);
    // The run's own actor cancels it, unless the loop already carried out the
    // pending cancel with its own authority.
    let (status, answer) = fleet
        .call("POST", &cancel, json!({"actor":Fleet::ai()}))
        .await;
    assert!(
        status == StatusCode::OK || answer["code"] == "run_not_active",
        "{status} {answer}"
    );
    let withdrawn = fleet
        .ok(
            "GET",
            &format!("/runs/{}", redispatched["run_id"].as_str().unwrap()),
            Value::Null,
        )
        .await;
    assert_eq!(withdrawn["state"], "cancelled", "{withdrawn}");
    assert!(!peer_ticket().up_next);
    let tick = fleet
        .ok("POST", "/scheduler/tick", json!({"actor":Fleet::ai()}))
        .await;
    assert_eq!(tick["mode"], "applied", "{tick}");
    let runs = fleet.runs().await;
    assert_eq!(runs.len(), 2, "{runs:#?}");
    assert!(
        runs.iter().all(|run| run["state"] == "cancelled"),
        "{runs:#?}"
    );

    // The orchestrator released no claim: the worker let go itself. Its session is
    // wound down by the next pass that reads its host at a safe point (a remote
    // host's turn end does not wake this loop yet, HS2-257Q2Z).
    assert_eq!(peer_ticket().claimed_by, None);
    assert_eq!(fleet.sessions().await.len(), 1);
    fleet.shut_down().await;
}

/// HS2-2EY9DP priority 4: duplicate requests. Automatic placement is enabled while
/// manual passes run through HTTP and the stdio MCP tool at the same moment, and the
/// scheduler's own operation ids are replayed by hand: one run, one session and one
/// terminal for the step, whoever asks how often.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn hs2_2ey9dp_duplicate_requests_never_dispatch_or_start_twice() {
    let mut fleet = Fleet::new().await;
    let orchestration = fleet.orchestration_ticket("Coordinate under duplicates");
    let work = fleet.target_ticket(Side::Controller, "Synthetic change asked for twice");
    let step = fleet
        .plan(&orchestration, Side::Controller, &work, json!([]))
        .await["step"]["step_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (origin, listener) = fleet.serve_plain().await;

    // The loop starts with the policy while passes are asked for concurrently.
    let manual = |fleet: &Fleet| {
        let router = fleet.router.clone();
        let path = format!("{}/scheduler/tick", fleet.base);
        tokio::spawn(async move {
            super::orchestration::api(router, "POST", &path, json!({"actor":Fleet::ai()}), true)
                .await
        })
    };
    let passes: Vec<_> = (0..4).map(|_| manual(&fleet)).collect();
    fleet.automatic().await;
    let more: Vec<_> = (0..4).map(|_| manual(&fleet)).collect();
    let by_mcp = {
        let (home, owner, origin) = (fleet.home.clone(), fleet.owner.id.clone(), origin.clone());
        tokio::task::spawn_blocking(move || {
            (0..2)
                .map(|_| {
                    mcp_named(
                        &home,
                        &["--server", &origin, "--secret", SECRET],
                        "hotsheet_targets",
                        json!({"action":"scheduler_tick","checkout":owner,
                            "actor_role":"ai","actor_id":super::e2e::ACTOR}),
                    )
                })
                .collect::<Vec<_>>()
        })
    };
    for pass in passes.into_iter().chain(more) {
        let (status, body) = pass.await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    for tick in by_mcp.await.unwrap() {
        assert!(
            tick["mode"] == "applied" || tick["mode"] == "dry_run",
            "{tick}"
        );
    }
    let a = placed(&fleet, &step, Side::Controller).await;

    // Replaying the scheduler's own operation ids by hand returns what they made.
    let run = fleet.mcp(
        &origin,
        json!({"action":"run_dispatch","target_id":super::e2e::TARGET_A,
            "ticket":{"native_id":orchestration.slug},"connection_id":fleet.connection(Side::Controller),
            "native_id":work.slug,"operation_id":format!("sched-run-{step}"),"mode":"dispatcher"}),
    );
    assert_eq!(run["run_id"], a.run_id.as_str(), "{run}");
    let (status, conflict) = fleet
        .call(
            "POST",
            "/runs",
            json!({"ticket":{"native_id":orchestration.slug},"target_id":super::e2e::TARGET_A,
                "connection_id":fleet.connection(Side::Controller),"native_id":work.slug,
                "operation_id":format!("sched-run-{step}"),"worker_id":"someone-else",
                "actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    let session = fleet
        .ok(
            "POST",
            "/sessions",
            json!({"target_id":super::e2e::TARGET_A,"operation_id":format!("sched-start-{step}"),
                "tool":super::e2e::TOOL_A,"run_id":a.run_id,"actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(session["session_id"], a.session_id.as_str(), "{session}");

    // One start is logged, naming the one session, run and operation: passes of
    // one owner run one at a time (HS2-7Q428Z).
    let log = fleet.decisions().await;
    let named: Vec<_> = log
        .iter()
        .filter(|entry| started(entry))
        .map(|entry| {
            (
                entry["session_id"].clone(),
                entry["run_id"].clone(),
                entry["operation_id"].clone(),
            )
        })
        .collect();
    assert_eq!(
        named,
        [(
            json!(a.session_id),
            json!(a.run_id),
            json!(format!("sched-start-{step}"))
        )],
        "{log:#?}"
    );
    assert_eq!(fleet.runs().await.len(), 1);
    assert_eq!(fleet.sessions().await.len(), 1);
    assert_eq!(
        fleet
            .drawer(Side::Controller)
            .await
            .iter()
            .filter(|tab| tab["alive"] == true)
            .count(),
        1
    );
    listener.stop().await;
    fleet.shut_down().await;
}

/// HS2-2EY9DP priority 4: events out of order and replayed. A fast worker claims,
/// finishes and releases between two of the follower's reads, and every source is
/// then asked again. The run settles once, from the target's latest state, and a
/// replayed fact records nothing more.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_2ey9dp_out_of_order_and_replayed_events_settle_a_run_once() {
    let mut fleet = Fleet::new().await;
    let (step, ticket, b) = one_remote_step(&mut fleet, "Synthetic change finished at once").await;
    // The peer's dispatcher is quicker than the follower: the ticket is released by its
    // worker, taken by another and completed while the controller cannot read the peer.
    fleet.peer_offline().await;
    let router = fleet.peer_api.clone();
    let release = super::orchestration::api(
        router.clone(),
        "POST",
        &format!("/tickets/{}/release", ticket.id),
        json!({"worker": b.worker}),
        true,
    )
    .await;
    assert_eq!(release.0, StatusCode::OK, "{}", release.1);
    fleet.claim(Side::Peer, &ticket, "another-worker").await;
    fleet.finish(Side::Peer, &ticket, "another-worker").await;
    fleet.peer_online().await;
    let done = fleet
        .run_until(&b.run_id, "completed", |status| {
            status["state"] == "completed"
        })
        .await;
    let states: Vec<_> = done["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["to"].as_str().unwrap().to_owned())
        .collect();
    // It completes from the latest state; the missed hand-over is not invented.
    assert_eq!(
        states.last().map(String::as_str),
        Some("completed"),
        "{done}"
    );
    assert_eq!(
        states.iter().filter(|state| *state == "completed").count(),
        1,
        "{done}"
    );
    let events = |fleet: &Fleet| {
        let path = format!("/run-events?after=0&run={}&limit=500", b.run_id);
        let fleet_router = fleet.router.clone();
        let base = fleet.base.clone();
        async move {
            super::orchestration::api(
                fleet_router,
                "GET",
                &format!("{base}{path}"),
                Value::Null,
                true,
            )
            .await
            .1["entries"]
                .as_array()
                .unwrap()
                .iter()
                // What a replayed fact could duplicate. Session events of the run's
                // session may still arrive from the loop meanwhile.
                .filter(|entry| entry["kind"] != "session")
                .map(|entry| format!("{} {} {}", entry["kind"], entry["state"], entry["reason"]))
                .collect::<Vec<_>>()
        }
    };
    let before = events(&fleet).await;
    // Every source is asked again: nothing more is recorded.
    for _ in 0..2 {
        fleet
            .ok(
                "POST",
                &format!("/runs/{}/sync", b.run_id),
                json!({"actor":Fleet::ai()}),
            )
            .await;
        fleet
            .ok("POST", "/run-reconcile", json!({"actor":Fleet::ai()}))
            .await;
    }
    assert_eq!(events(&fleet).await, before);
    fleet.step_until(&step, "finished").await;
    fleet.shut_down().await;
}

/// HS2-2EY9DP: a remote session whose work ends without any run event (its run was
/// cancelled and its step removed, then its worker let go) idles and is wound down.
/// HS2-257Q2Z: the peer's claim release and turn end reach the controller on the
/// target change feed its follower holds open, which wakes the owner's loop.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_2ey9dp_a_remote_session_freed_without_a_run_event_is_wound_down() {
    let mut fleet = Fleet::new().await;
    let (step, ticket, b) = one_remote_step(&mut fleet, "Synthetic change let go").await;
    fleet
        .ok(
            "POST",
            "/readiness/remove",
            json!({"actor":Fleet::person(),"step_id":step}),
        )
        .await;
    let cancelled = fleet
        .ok(
            "POST",
            &format!("/runs/{}/cancel", b.run_id),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(cancelled["state"], "cancelled", "{cancelled}");
    // HS2-257Q2Z: the peer's hosted feed, read as the controller's follower reads it.
    let feed = format!("/checkouts/{}/target-events", fleet.b_checkout.id);
    let (status, handshake) =
        super::orchestration::api(fleet.peer_api.clone(), "GET", &feed, Value::Null, true).await;
    assert_eq!(status, StatusCode::OK, "{handshake}");
    let mut cursor = handshake["cursor"].as_u64().unwrap();
    fleet.finish(Side::Peer, &ticket, &b.worker).await;
    let stop = fleet
        .decided(
            "the freed remote session wound down",
            |entry| {
                entry["kind"] == "wind_down"
                    && entry["outcome"] == "wind_down_requested"
                    && entry["session_id"] == b.session_id.as_str()
            },
            3 * MINUTE,
        )
        .await;
    assert_eq!(stop["reason"], "idle", "{stop}");
    // The wound-down terminal's exit is published on the peer's feed for its checkout,
    // naming the host's terminal and nothing else.
    let deadline = std::time::Instant::now() + MINUTE;
    let exited = loop {
        let (status, page) = super::orchestration::api(
            fleet.peer_api.clone(),
            "GET",
            &format!("{feed}?since={cursor}&wait_ms=8000"),
            Value::Null,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        cursor = page["cursor"].as_u64().unwrap();
        let named = page["sessions"].as_array().is_some_and(|sessions| {
            sessions
                .iter()
                .any(|event| event["session_id"] == b.terminal.as_str())
        });
        if named {
            break page;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no session change on the peer's feed"
        );
    };
    for event in exited["sessions"].as_array().unwrap() {
        let mut keys: Vec<&String> = event.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, ["cursor", "session_id"], "{event}");
    }
    fleet.shut_down().await;
}

/// HS2-2EY9DP: a refusing, low-capacity peer and a limited tool. The peer's owner set
/// its machine to admit no session and the remote target's preferred tool is
/// rate-limited. The loop places nothing and backs off; when the owner lifts the
/// limit, the next pass after the backoff starts the session on the other allowed
/// tool, with the scheduler's operation id, on the peer.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_2ey9dp_a_refusing_peer_and_a_limited_tool_are_waited_out_and_failed_over() {
    let mut fleet = Fleet::new().await;
    // Target B may run either tool; the Codex-like one is preferred by weight.
    fleet
        .grant_tools(
            TARGET_B,
            &[super::e2e::TOOL_B, super::e2e::TOOL_A],
            super::session_work::now_ms() + 3_600_000,
        )
        .await;
    let policy = json!({
        "tools":[{"tool":super::e2e::TOOL_A,"max_sessions":4,"weight":10},
            {"tool":super::e2e::TOOL_B,"max_sessions":4,"weight":90}],
        "targets":[
            {"target_id":super::e2e::TARGET_A,"actions":super::e2e::ACTIONS,
                "tools":[{"tool":super::e2e::TOOL_A}],"max_sessions":1},
            {"target_id":TARGET_B,"actions":super::e2e::ACTIONS,
                "tools":[{"tool":super::e2e::TOOL_B},{"tool":super::e2e::TOOL_A}],
                "max_sessions":1}]});
    fleet.policy(policy.clone()).await;
    // The peer's owner admits no session on their machine: a person's act there, by
    // the CLI, that no orchestrator route can loosen.
    let peer_owner = [
        "--actor-role",
        "human",
        "--actor-id",
        "peer-owner",
        "orchestration",
        "admission",
    ];
    let closed = cli_named(
        &fleet.peer_home,
        &peer_owner,
        &["set", "--max-sessions", "0"],
    );
    assert_eq!(closed["admission"]["max_sessions"], 0, "{closed}");
    // The preferred tool reports a rate limit that lasts the whole scenario.
    let now = super::session_work::now_ms();
    let limited = fleet
        .ok(
            "POST",
            "/tool-capacity/signals",
            json!({"tool":super::e2e::TOOL_B,"kind":"rate_limit","at_ms":now,
                "resets_at_ms":now + 3 * 3_600_000,"actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(limited["state"], "limited", "{limited}");

    let orchestration = fleet.orchestration_ticket("Coordinate on a crowded peer");
    let ticket = fleet.target_ticket(Side::Peer, "Synthetic change for a crowded peer");
    let step = fleet
        .plan(&orchestration, Side::Peer, &ticket, json!([]))
        .await["step"]["step_id"]
        .as_str()
        .unwrap()
        .to_owned();
    policy_automatic(&mut fleet, policy).await;

    // The loop fails over to the tool that is not limited and asks the peer, whose
    // own admission refuses: the controller cannot see the owner's file, only the
    // answer. Nothing starts, the start is recorded as failed with the host's reason,
    // and the target is left alone for the backoff.
    let refused = fleet
        .decided(
            "a refused start on the peer",
            |entry| {
                entry["kind"] == "start"
                    && entry["step_id"] == step.as_str()
                    && entry["outcome"] == "refused"
            },
            MINUTE,
        )
        .await;
    assert_eq!(refused["tool"], super::e2e::TOOL_A, "{refused}");
    let backoff = fleet
        .decided(
            "the refusal backoff",
            |entry| {
                entry["kind"] == "unplaced"
                    && entry["step_id"] == step.as_str()
                    && entry["reason"] == "refusal_backoff"
            },
            MINUTE,
        )
        .await;
    assert_eq!(backoff["detail"], "session_cap_reached", "{backoff}");
    assert!(fleet.drawer(Side::Peer).await.is_empty());
    let failed = fleet.sessions().await;
    assert_eq!(failed.len(), 1, "{failed:#?}");
    assert_eq!(failed[0]["state"], "failed", "{failed:#?}");
    assert_eq!(
        failed[0]["history"].as_array().unwrap().last().unwrap()["reason"],
        "session_cap_reached"
    );

    // The owner opens the machine again. After the backoff the loop starts the session
    // on the tool that is not limited.
    cli_named(
        &fleet.peer_home,
        &peer_owner,
        &["set", "--clear", "max_sessions"],
    );
    let start = fleet.started_for(&step, 3 * MINUTE).await;
    assert_eq!(start["tool"], super::e2e::TOOL_A, "{start}");
    let (session, terminal, _) = fleet.started_session(&start).await;
    assert_eq!(session["tool"], super::e2e::TOOL_A, "{session}");
    // A replacement of the refused start, under the next operation id.
    assert_eq!(
        session["operation_id"],
        format!("sched-start-{step}.2").as_str()
    );
    assert_eq!(session["replaces"], failed[0]["session_id"], "{session}");
    assert!(
        fleet
            .drawer(Side::Peer)
            .await
            .iter()
            .any(|tab| tab["id"] == terminal.as_str() && tab["tool"] == super::e2e::TOOL_A)
    );
    fleet.shut_down().await;
}

async fn policy_automatic(fleet: &mut Fleet, mut policy: Value) {
    policy["placement"] = json!({"automatic":true,"actor":super::e2e::ACTOR,"cooldown_minutes":0});
    fleet.policy(policy).await;
}

/// HS2-2EY9DP: the controller restarts while its session's run waits on CI, and the
/// session dies with it. Reconciliation finds no work lost (`waiting_on_gate`); the
/// loop should agree and start no replacement while the gate waits.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_2ey9dp_a_session_lost_while_its_run_waits_on_ci_is_not_replaced() {
    let mut fleet = Fleet::new().await;
    let orchestration = fleet.orchestration_ticket("Coordinate a change waiting on CI");
    let work = fleet.target_ticket(Side::Controller, "Synthetic change waiting on CI");
    let step = fleet
        .plan(&orchestration, Side::Controller, &work, json!([]))
        .await["step"]["step_id"]
        .as_str()
        .unwrap()
        .to_owned();
    fleet.automatic().await;
    let a = placed(&fleet, &step, Side::Controller).await;
    works(&fleet, Side::Controller, &a, &work).await;
    let commit = "c".repeat(40);
    fleet.provider.run(71, &commit, None);
    fleet
        .ok(
            "POST",
            &format!("/runs/{}/gates", a.run_id),
            json!({"commit":commit,"repository":"acme/widgets","actor":Fleet::ai()}),
        )
        .await;
    fleet.restart_controller().await;
    let reconciled = fleet
        .ok(
            "POST",
            &format!("/runs/{}/reconcile", a.run_id),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    let session_finding = reconciled["reconciliation"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["source"] == "session")
        .cloned()
        .unwrap();
    assert_eq!(session_finding["reason"], "waiting_on_gate", "{reconciled}");
    let tick = fleet
        .ok("POST", "/scheduler/tick", json!({"actor":Fleet::ai()}))
        .await;
    assert_eq!(tick["mode"], "applied", "{tick}");
    let log = starts(&fleet).await;
    assert_eq!(log.len(), 1, "{log:#?}");
    assert_eq!(fleet.sessions().await.len(), 1);
    fleet.shut_down().await;
}
