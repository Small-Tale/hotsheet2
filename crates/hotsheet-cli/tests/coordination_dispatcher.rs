//! Real separate-process dispatcher, integrator and worker boundaries (HS2-TWZW7X).
//! Every pool here is a disposable, independently initialized project and ticket store.

#[path = "support/coordination.rs"]
mod support;

use std::fs;

use hotsheet_cli::coordination::{Authority, Phase};
use hotsheet_ticketing::git_connection_id;
use support::{Fixture, git};

const WORKER: [&str; 7] = [
    "next",
    "--role",
    "worker",
    "--id",
    "worker",
    "--session",
    "worker-session",
];

fn dispatcher_args<'a>(session: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        "--role",
        "dispatcher",
        "--id",
        "dispatcher",
        "--session",
        session,
        "--route",
        "human",
        "--lease-seconds",
        "600",
    ];
    args.extend_from_slice(extra);
    args
}

fn integrator(f: &Fixture, command: &str, extra: &[&str]) -> hotsheet_cli::coordination::Snapshot {
    let mut args = vec![command, "--id", "integrator", "--session", "root-one"];
    args.extend_from_slice(extra);
    f.run(&args)
}

fn register_integrator(f: &Fixture) {
    f.run(&[
        "register",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root-one",
        "--route",
        "native:/root",
        "--lease-seconds",
        "600",
    ]);
}

fn enable_dispatcher(f: &Fixture, base: &str) {
    register_integrator(f);
    integrator(
        f,
        "authority",
        &["--mode", "dispatcher", "--all-participants-upgraded"],
    );
    integrator(f, "publish", &["--base", base]);
}

fn dispatch(
    f: &Fixture,
    id: &str,
    session: &str,
    operation: &str,
    ticket: &hotsheet_model::Ticket,
    base: &str,
    scope: &str,
) -> std::process::Output {
    let connection = git_connection_id(&f.store);
    let native = ticket.id.to_string();
    let args = f.coord(&[
        "assign",
        "--id",
        id,
        "--session",
        session,
        "--worker",
        "worker",
        "--worker-session",
        "worker-session",
        "--operation",
        operation,
        "--connection",
        &connection,
        "--native-id",
        &native,
        "--slug",
        &ticket.slug,
        "--base",
        base,
        "--scope",
        scope,
    ]);
    f.command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap()
}

fn refused(output: &std::process::Output, reason: &str) {
    assert!(!output.status.success(), "expected refusal: {reason}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(reason), "{stderr}");
}

fn succeeded(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn hs2_twzw7x_dispatcher_started_first_waits_for_authority_then_refills_during_held_integration() {
    let f = Fixture::new();
    let first = f.ticket();
    let second = f.ticket();
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    // Arbitrary startup: the dispatcher waits on a legacy pool without registering.
    let mut dispatcher =
        f.waiting(&[&["next"][..], &dispatcher_args("dispatch-one", &[])].concat());
    let mut worker = f.waiting(&WORKER);
    let legacy = f.run(&["status"]);
    assert!(legacy.state.dispatcher.is_none());
    assert_eq!(legacy.state.version, 3);
    assert!(
        legacy
            .dispatch_blockers
            .contains(&"assignment_authority_is_integrator".to_owned())
    );
    register_integrator(&f);
    integrator(
        &f,
        "authority",
        &["--mode", "dispatcher", "--all-participants-upgraded"],
    );
    // The genuine authority event wakes it; it reports the missing safe base, not an empty queue.
    let woke = dispatcher.result();
    assert!(woke.live_dispatcher);
    assert_eq!(woke.state.authority, Authority::Dispatcher);
    assert!(
        woke.dispatch_blockers
            .contains(&"no_safe_base_published".to_owned())
    );
    refused(
        &dispatch(
            &f,
            "dispatcher",
            "dispatch-one",
            "op-1",
            &first,
            &base,
            "synthetic.txt",
        ),
        "not published a safe shipping base",
    );
    integrator(&f, "publish", &["--base", &base]);
    refused(
        &dispatch(
            &f,
            "integrator",
            "root-one",
            "op-1",
            &first,
            &base,
            "synthetic.txt",
        ),
        "assignment authority is the dispatcher",
    );
    succeeded(&dispatch(
        &f,
        "dispatcher",
        "dispatch-one",
        "op-1",
        &first,
        &base,
        "synthetic.txt",
    ));
    let offered = worker.result();
    let assignment = offered.state.active_assignment("worker").unwrap();
    assert_eq!(assignment.ticket.native_id, first.id.to_string());
    // Handoffs return to the integrator, which reviews them.
    assert_eq!(
        assignment.return_route.clone(),
        offered.state.integrator.unwrap().return_route
    );

    f.cli(&["claim", &first.slug, "--worker", "worker", "--eta", "10m"]);
    f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "op-1",
        "--phase",
        "accepted",
    ]);
    let checkout = f.root.path().join("worker-one");
    git(
        &f.project,
        &[
            "worktree",
            "add",
            "-qb",
            "worker-one",
            checkout.to_str().unwrap(),
            &base,
        ],
    );
    fs::write(
        checkout.join("synthetic.txt"),
        "dispatched synthetic change\n",
    )
    .unwrap();
    git(&checkout, &["add", "synthetic.txt"]);
    git(
        &checkout,
        &["commit", "-qm", "HS-SYNTHETIC Change synthetic fixture"],
    );
    let commit = git(&checkout, &["rev-parse", "HEAD"]);
    f.cli(&[
        "edit",
        &first.slug,
        "--note",
        &format!("## Integration handoff\nSynthetic commit {commit}"),
    ]);
    f.cli(&["release", &first.slug, "--worker", "worker"]);
    f.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "op-1",
        "--phase",
        "handoff",
        "--commit",
        &commit,
    ]);
    // Integration is held (serial review/gates) while the dispatcher refills the worker.
    let mut again = f.waiting(&WORKER);
    integrator(
        &f,
        "mark",
        &["--assignment", "op-1", "--phase", "integrating"],
    );
    refused(
        &dispatch(
            &f,
            "dispatcher",
            "dispatch-one",
            "op-2",
            &second,
            &base,
            "synthetic.txt",
        ),
        "held by pending assignment op-1",
    );
    succeeded(&dispatch(
        &f,
        "dispatcher",
        "dispatch-one",
        "op-2",
        &second,
        &base,
        "other.txt",
    ));
    let refilled = again.result();
    assert_eq!(
        refilled.state.active_assignment("worker").unwrap().id,
        "op-2"
    );
    assert_eq!(refilled.state.assignments["op-1"].phase, Phase::Integrating);
    let ci = integrator(&f, "mark", &["--assignment", "op-1", "--phase", "ci"]);
    assert_eq!(ci.state.assignments["op-1"].phase, Phase::Ci);
    assert_eq!(ci.state.assignments["op-2"].phase, Phase::Offered);
}

#[test]
fn hs2_twzw7x_duplicate_and_stale_dispatchers_are_fenced_and_lost_delivery_replays() {
    let f = Fixture::new();
    let ticket = f.ticket();
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    enable_dispatcher(&f, &base);
    let mut worker = f.waiting(&WORKER);
    let cursor = f
        .run(&[&["register"][..], &dispatcher_args("dispatch-one", &[])].concat())
        .cursor;
    let held = f.waiting(
        &[
            &["next"][..],
            &dispatcher_args("dispatch-one", &["--after", &cursor]),
        ]
        .concat(),
    );
    let duplicate = f.coord(&[&["next"][..], &dispatcher_args("dispatch-one", &[])].concat());
    refused(
        &f.command(&duplicate.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap(),
        "another active wait owns this session",
    );
    let takeover = f.coord(&[&["register"][..], &dispatcher_args("dispatch-two", &[])].concat());
    refused(
        &f.command(&takeover.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap(),
        "another live session holds this role",
    );
    refused(
        &dispatch(
            &f,
            "dispatcher",
            "dispatch-two",
            "op-1",
            &ticket,
            &base,
            "synthetic.txt",
        ),
        "exact dispatcher session",
    );
    succeeded(&dispatch(
        &f,
        "dispatcher",
        "dispatch-one",
        "op-1",
        &ticket,
        &base,
        "synthetic.txt",
    ));
    // A lost cue replays the same durable reservation without another assignment.
    succeeded(&dispatch(
        &f,
        "dispatcher",
        "dispatch-one",
        "op-1",
        &ticket,
        &base,
        "synthetic.txt",
    ));
    let status = f.run(&["status"]);
    assert_eq!(status.state.assignments.len(), 1);
    assert_eq!(
        worker
            .result()
            .state
            .active_assignment("worker")
            .unwrap()
            .id,
        "op-1"
    );
    drop(held);
    // The dispatcher's explicit leave lets the integrator take authority back, still schema four.
    f.run(
        &[
            &["leave"][..],
            &[
                "--role",
                "dispatcher",
                "--id",
                "dispatcher",
                "--session",
                "dispatch-one",
            ],
        ]
        .concat(),
    );
    let returned = integrator(&f, "authority", &["--mode", "integrator"]);
    assert_eq!(
        (returned.state.version, returned.state.authority),
        (4, Authority::Integrator)
    );
}

#[test]
fn hs2_twzw7x_dispatcher_refuses_dependencies_blockers_and_unsupported_sources() {
    let f = Fixture::new();
    let dependency = f.ticket();
    let dependent = f.ticket();
    let blocked = f.ticket();
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    enable_dispatcher(&f, &base);
    let blocked_ref = format!("{}:{}", git_connection_id(&f.store), blocked.id);
    integrator(&f, "publish", &["--base", &base, "--blocked", &blocked_ref]);
    f.run(&[&["register"][..], &dispatcher_args("dispatch-one", &[])].concat());
    f.run(&[
        "register",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--lease-seconds",
        "600",
    ]);
    f.cli(&["edit", &dependent.slug, "--blocked-by", &dependency.slug]);
    refused(
        &dispatch(
            &f,
            "dispatcher",
            "dispatch-one",
            "op-dep",
            &dependent,
            &base,
            "a",
        ),
        "depends on",
    );
    refused(
        &dispatch(
            &f,
            "dispatcher",
            "dispatch-one",
            "op-blocked",
            &blocked,
            &base,
            "a",
        ),
        "blocked this ticket",
    );
    let foreign = f.coord(&[
        "assign",
        "--id",
        "dispatcher",
        "--session",
        "dispatch-one",
        "--worker",
        "worker",
        "--worker-session",
        "worker-session",
        "--operation",
        "op-foreign",
        "--connection",
        "github-synthetic",
        "--native-id",
        "7",
        "--slug",
        "GH-7",
        "--base",
        &base,
        "--scope",
        "a",
    ]);
    refused(
        &f.command(&foreign.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap(),
        "unsupported for this connection",
    );
    // Closing the dependency makes the dependent assignable.
    f.cli(&[
        "edit",
        &dependency.slug,
        "--status",
        "completed",
        "--note",
        "synthetic done",
    ]);
    succeeded(&dispatch(
        &f,
        "dispatcher",
        "dispatch-one",
        "op-dep",
        &dependent,
        &base,
        "a",
    ));
    assert!(f.run(&["status"]).state.assignments.contains_key("op-dep"));
}

#[test]
fn hs2_twzw7x_capabilities_never_open_and_legacy_pools_stay_schema_three() {
    let f = Fixture::new();
    let ticket = f.ticket();
    let capabilities = f.cli(
        &f.coord(&["capabilities"])
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    let reported: serde_json::Value = serde_json::from_str(&capabilities).unwrap();
    assert_eq!(reported["schemas_read"], serde_json::json!([1, 2, 3, 4, 5]));
    assert_eq!(reported["schema_written_with_integrator_authority"], 3);
    // HS2-YHYNJ6: reviewed shared sections are schema five, reported without opening.
    assert_eq!(reported["schema_written_with_shared_sections"], 5);
    assert_eq!(
        reported["scope_section_kinds"],
        serde_json::json!(["row", "heading"])
    );
    assert!(!f.project.join(".git/hotsheet-coordination").exists());

    let refusal = f.coord(&[&["register"][..], &dispatcher_args("dispatch-one", &[])].concat());
    refused(
        &f.command(&refusal.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap(),
        "assignment authority is the integrator",
    );
    register_integrator(&f);
    f.run(&[
        "register",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--lease-seconds",
        "600",
    ]);
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    refused(
        &dispatch(
            &f,
            "integrator",
            "root-one",
            "op-scoped",
            &ticket,
            &base,
            "a",
        ),
        "scopes require dispatcher authority",
    );
    f.assign("op-legacy", &ticket, &base, "root-one");
    let raw: serde_json::Value = serde_json::from_slice(
        &fs::read(f.project.join(".git/hotsheet-coordination/state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(raw["version"], 3);
    for field in ["authority", "dispatcher", "integration"] {
        assert!(raw.get(field).is_none(), "{field} leaked into schema three");
    }
    assert!(raw["assignments"]["op-legacy"].get("scope").is_none());
}

/// Consumed CPU seconds of a live child process, from `ps` (macOS and Linux formats).
#[cfg(unix)]
fn cpu_seconds(pid: u32) -> f64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "time=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let text = text.trim();
    let (days, clock) = text
        .split_once('-')
        .map_or((0.0, text), |(d, c)| (d.parse().unwrap(), c));
    let seconds = clock.split(':').fold(0.0, |total, part| {
        total * 60.0 + part.parse::<f64>().unwrap()
    });
    days * 86_400.0 + seconds
}

#[cfg(unix)]
#[test]
fn hs2_twzw7x_unregistered_dispatcher_idles_past_half_lease_and_peer_expiry_then_continues() {
    let f = Fixture::new();
    let ticket = f.ticket();
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    // A short-lease peer expires during the wait; the dispatcher's own half-lease (5 s) also passes.
    f.run(&[
        "register",
        "--role",
        "worker",
        "--id",
        "short",
        "--session",
        "short-session",
        "--lease-seconds",
        "10",
    ]);
    let mut dispatcher = f.waiting(
        &[
            &["next"][..],
            &dispatcher_args("dispatch-one", &[])[..8],
            &["--lease-seconds", "10"],
        ]
        .concat(),
    );
    std::thread::sleep(std::time::Duration::from_secs(13));
    let idle_cpu = cpu_seconds(dispatcher.child.id());
    assert!(
        idle_cpu < 1.0,
        "unregistered dispatcher used {idle_cpu}s CPU while idle for 13s; it must block on events"
    );
    assert!(
        dispatcher.child.try_wait().unwrap().is_none(),
        "the waiter is still waiting"
    );
    assert!(f.run(&["status"]).state.dispatcher.is_none());
    // The same waiter now receives the authority switch and registers.
    register_integrator(&f);
    integrator(
        &f,
        "authority",
        &["--mode", "dispatcher", "--all-participants-upgraded"],
    );
    let woke = dispatcher.result();
    assert_eq!(woke.state.dispatcher.unwrap().generation, "dispatch-one");
    integrator(&f, "publish", &["--base", &base]);
    f.run(&[
        "register",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--lease-seconds",
        "600",
    ]);
    succeeded(&dispatch(
        &f,
        "dispatcher",
        "dispatch-one",
        "op-1",
        &ticket,
        &base,
        "a",
    ));
}

#[test]
fn hs2_twzw7x_reopened_dependency_is_refused_at_the_reservation_boundary() {
    let f = Fixture::new();
    let dependency = f.ticket();
    let dependent = f.ticket();
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    f.cli(&["edit", &dependent.slug, "--blocked-by", &dependency.slug]);
    f.cli(&[
        "edit",
        &dependency.slug,
        "--status",
        "completed",
        "--note",
        "synthetic done",
    ]);
    // Legacy authority does not apply the dispatcher dependency rule, but cannot use scopes.
    register_integrator(&f);
    f.run(&[
        "register",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--lease-seconds",
        "600",
    ]);
    // Switch to dispatcher authority, then reopen the dependency before reserving.
    integrator(
        &f,
        "authority",
        &["--mode", "dispatcher", "--all-participants-upgraded"],
    );
    integrator(&f, "publish", &["--base", &base]);
    f.run(&[&["register"][..], &dispatcher_args("dispatch-one", &[])].concat());
    f.cli(&["edit", &dependency.slug, "--status", "not_started"]);
    refused(
        &dispatch(
            &f,
            "dispatcher",
            "dispatch-one",
            "op-dep",
            &dependent,
            &base,
            "a",
        ),
        "depends on",
    );
    refused(
        &dispatch(
            &f,
            "integrator",
            "root-one",
            "op-dep",
            &dependent,
            &base,
            "a",
        ),
        "assignment authority is the dispatcher",
    );
    f.cli(&[
        "edit",
        &dependency.slug,
        "--status",
        "completed",
        "--note",
        "synthetic done again",
    ]);
    succeeded(&dispatch(
        &f,
        "dispatcher",
        "dispatch-one",
        "op-dep",
        &dependent,
        &base,
        "a",
    ));
    assert_eq!(f.run(&["status"]).state.assignments.len(), 1);
}

/// `(worker, worker generation)` the evidence and reservation address.
type Addressed<'a> = (&'a str, &'a str);

fn route_evidence(
    f: &Fixture,
    ticket: &hotsheet_model::Ticket,
    operation: &str,
    (worker, generation): Addressed<'_>,
    route: &str,
    author: &str,
) -> String {
    let json = serde_json::json!({
        "operation": operation, "worker": worker, "generation": generation, "route": route,
    });
    f.cli(&[
        "edit",
        &ticket.slug,
        "--note",
        &format!("## Assignment return route\n\n{json}"),
        "--note-kind",
        "regular",
        "--actor-role",
        "ai",
        "--actor-id",
        author,
    ]);
    f.store
        .read_ticket(&ticket.id)
        .unwrap()
        .notes
        .last()
        .unwrap()
        .id
        .to_string()
}

struct Reservation<'a> {
    authority: (&'a str, &'a str),
    operation: &'a str,
    worker: Addressed<'a>,
    scope: Option<&'a str>,
    route: Option<(&'a str, &'a str)>,
}

fn routed(
    f: &Fixture,
    ticket: &hotsheet_model::Ticket,
    base: &str,
    r: Reservation<'_>,
) -> std::process::Output {
    let connection = git_connection_id(&f.store);
    let native = ticket.id.to_string();
    let mut args = vec![
        "assign",
        "--id",
        r.authority.0,
        "--session",
        r.authority.1,
        "--worker",
        r.worker.0,
        "--worker-session",
        r.worker.1,
        "--operation",
        r.operation,
        "--connection",
        &connection,
        "--native-id",
        &native,
        "--slug",
        &ticket.slug,
        "--base",
        base,
    ];
    if let Some(scope) = r.scope {
        args.extend(["--scope", scope]);
    }
    if let Some((route, note)) = r.route {
        args.extend(["--return-route", route, "--route-evidence-note", note]);
    }
    let args = f.coord(&args);
    f.command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap()
}

#[test]
fn hs2_twzw7x_route_evidence_is_authored_by_the_active_assignment_authority() {
    use hotsheet_cli::coordination::ReturnRoute;
    let f = Fixture::new();
    let legacy = f.ticket();
    let dispatched = f.ticket();
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    let (first, second) = (("worker", "worker-session"), ("second", "second-session"));
    register_integrator(&f);
    for (id, session) in [first, second] {
        f.run(&[
            "register",
            "--role",
            "worker",
            "--id",
            id,
            "--session",
            session,
            "--lease-seconds",
            "600",
        ]);
    }
    let root = ("integrator", "root-one");
    let dispatcher = ("dispatcher", "dispatch-one");
    let tty = "terminal:worker-tty";
    let reserve = |authority, operation, worker, scope, route| Reservation {
        authority,
        operation,
        worker,
        scope,
        route,
    };

    // Legacy: only the integrator authors evidence for a new operation.
    let by_dispatcher = route_evidence(&f, &legacy, "op-legacy", first, tty, "dispatcher");
    refused(
        &routed(
            &f,
            &legacy,
            &base,
            reserve(root, "op-legacy", first, None, Some((tty, &by_dispatcher))),
        ),
        "exact active assignment authority",
    );
    let by_integrator = route_evidence(&f, &legacy, "op-legacy", first, tty, "integrator");
    succeeded(&routed(
        &f,
        &legacy,
        &base,
        reserve(root, "op-legacy", first, None, Some((tty, &by_integrator))),
    ));

    // Dispatcher authority: the dispatcher authors evidence for new operations.
    integrator(
        &f,
        "authority",
        &["--mode", "dispatcher", "--all-participants-upgraded"],
    );
    integrator(&f, "publish", &["--base", &base]);
    f.run(&[&["register"][..], &dispatcher_args("dispatch-one", &[])].concat());
    // The reserved legacy route is immutable across the authority change: a replay by the
    // new authority keeps it, and a different route is a different operation.
    succeeded(&routed(
        &f,
        &legacy,
        &base,
        reserve(dispatcher, "op-legacy", first, None, None),
    ));
    assert_eq!(
        f.run(&["status"]).state.assignments["op-legacy"].return_route,
        ReturnRoute::Terminal("worker-tty".into())
    );
    let other = route_evidence(&f, &legacy, "op-legacy", first, "human", "dispatcher");
    refused(
        &routed(
            &f,
            &legacy,
            &base,
            reserve(
                dispatcher,
                "op-legacy",
                first,
                None,
                Some(("human", &other)),
            ),
        ),
        "different assignment",
    );

    let by_root = route_evidence(&f, &dispatched, "op-new", second, tty, "integrator");
    refused(
        &routed(
            &f,
            &dispatched,
            &base,
            reserve(
                dispatcher,
                "op-new",
                second,
                Some("a"),
                Some((tty, &by_root)),
            ),
        ),
        "exact active assignment authority",
    );
    let stale = route_evidence(
        &f,
        &dispatched,
        "op-new",
        ("second", "stale-session"),
        tty,
        "dispatcher",
    );
    refused(
        &routed(
            &f,
            &dispatched,
            &base,
            reserve(dispatcher, "op-new", second, Some("a"), Some((tty, &stale))),
        ),
        "exact operation, worker, generation and route",
    );
    let good = route_evidence(&f, &dispatched, "op-new", second, tty, "dispatcher");
    refused(
        &routed(
            &f,
            &dispatched,
            &base,
            reserve(
                dispatcher,
                "op-new",
                second,
                Some("a"),
                Some(("human", &good)),
            ),
        ),
        "exact operation, worker, generation and route",
    );
    succeeded(&routed(
        &f,
        &dispatched,
        &base,
        reserve(dispatcher, "op-new", second, Some("a"), Some((tty, &good))),
    ));
    let state = f.run(&["status"]).state;
    assert_eq!(
        state.assignments["op-new"].return_route,
        ReturnRoute::Terminal("worker-tty".into())
    );
    assert_eq!(state.assignments.len(), 2);
}

/// Spawn a dispatcher `assign` without waiting for it, so it can block on a held lock.
fn spawn_dispatch(
    f: &Fixture,
    session: &str,
    operation: &str,
    ticket: &hotsheet_model::Ticket,
    base: &str,
) -> std::process::Child {
    let connection = git_connection_id(&f.store);
    let native = ticket.id.to_string();
    let args = f.coord(&[
        "assign",
        "--id",
        "dispatcher",
        "--session",
        session,
        "--worker",
        "worker",
        "--worker-session",
        "worker-session",
        "--operation",
        operation,
        "--connection",
        &connection,
        "--native-id",
        &native,
        "--slug",
        &ticket.slug,
        "--base",
        base,
        "--scope",
        "a",
    ]);
    f.command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap()
}

/// Hold the selected source's real ticket mutation lock until `release` is dropped or sent.
fn hold_ticket_lock(f: &Fixture) -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>) {
    let root = f.store.root().to_path_buf();
    let (held_tx, held) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        let store = hotsheet_ticketing::FsStore::open_without_maintenance(&root).unwrap();
        store
            .with_ticket_transaction(|| {
                held_tx.send(()).unwrap();
                let _ = released.recv();
                Ok::<(), hotsheet_ticketing::StoreError>(())
            })
            .unwrap();
    });
    held.recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    (release, holder)
}

fn register_with_lease(f: &Fixture, role: &str, id: &str, session: &str, lease: &str) {
    f.run(&[
        "register",
        "--role",
        role,
        "--id",
        id,
        "--session",
        session,
        "--lease-seconds",
        lease,
    ]);
}

#[test]
fn hs2_twzw7x_blocked_reservation_is_refused_after_its_dispatcher_expires_then_is_replaced() {
    let f = Fixture::new();
    let ticket = f.ticket();
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    enable_dispatcher(&f, &base);
    register_with_lease(&f, "worker", "worker", "worker-session", "600");
    register_with_lease(&f, "dispatcher", "dispatcher", "dispatch-one", "10");
    let registered = std::time::Instant::now();
    let (release, holder) = hold_ticket_lock(&f);
    let mut blocked = spawn_dispatch(&f, "dispatch-one", "op-1", &ticket, &base);
    std::thread::sleep(std::time::Duration::from_millis(1500));
    assert!(
        blocked.try_wait().unwrap().is_none(),
        "assign waits on the held ticket lock"
    );
    // Only the dispatcher's real availability lapses while the command is blocked.
    std::thread::sleep(std::time::Duration::from_secs(12).saturating_sub(registered.elapsed()));
    release.send(()).unwrap();
    holder.join().unwrap();
    let output = blocked.wait_with_output().unwrap();
    refused(&output, "exact dispatcher session");
    assert!(
        f.run(&["status"]).state.assignments.is_empty(),
        "no reservation after expiry"
    );
    // A replacement generation takes over; the expired one stays fenced even if it renews late.
    register_with_lease(&f, "dispatcher", "dispatcher", "dispatch-two", "600");
    refused(
        &dispatch(
            &f,
            "dispatcher",
            "dispatch-one",
            "op-1",
            &ticket,
            &base,
            "a",
        ),
        "exact dispatcher session",
    );
    let stale = f.coord(&[&["register"][..], &dispatcher_args("dispatch-one", &[])].concat());
    refused(
        &f.command(&stale.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap(),
        "another live session holds this role",
    );
    succeeded(&dispatch(
        &f,
        "dispatcher",
        "dispatch-two",
        "op-1",
        &ticket,
        &base,
        "a",
    ));
    assert_eq!(f.run(&["status"]).state.assignments.len(), 1);
}

#[test]
fn hs2_twzw7x_blocked_reservation_is_refused_after_its_worker_expires() {
    let f = Fixture::new();
    let ticket = f.ticket();
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    enable_dispatcher(&f, &base);
    register_with_lease(&f, "dispatcher", "dispatcher", "dispatch-one", "600");
    register_with_lease(&f, "worker", "worker", "worker-session", "10");
    let registered = std::time::Instant::now();
    let (release, holder) = hold_ticket_lock(&f);
    let mut blocked = spawn_dispatch(&f, "dispatch-one", "op-1", &ticket, &base);
    std::thread::sleep(std::time::Duration::from_millis(1500));
    assert!(
        blocked.try_wait().unwrap().is_none(),
        "assign waits on the held ticket lock"
    );
    std::thread::sleep(std::time::Duration::from_secs(12).saturating_sub(registered.elapsed()));
    release.send(()).unwrap();
    holder.join().unwrap();
    refused(
        &blocked.wait_with_output().unwrap(),
        "worker is not currently available",
    );
    assert!(f.run(&["status"]).state.assignments.is_empty());
}

#[test]
fn hs2_twzw7x_full_original_route_replay_survives_authority_and_identity_changes() {
    use hotsheet_cli::coordination::ReturnRoute;
    let f = Fixture::new();
    let ticket = f.ticket();
    let other = f.ticket();
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    let first = ("worker", "worker-session");
    register_integrator(&f);
    register_with_lease(&f, "worker", "worker", "worker-session", "600");
    let tty = "terminal:worker-tty";
    let original = route_evidence(&f, &ticket, "op-legacy", first, tty, "integrator");
    let full = |authority| Reservation {
        authority,
        operation: "op-legacy",
        worker: first,
        scope: None,
        route: Some((tty, original.as_str())),
    };
    succeeded(&routed(
        &f,
        &ticket,
        &base,
        full(("integrator", "root-one")),
    ));

    // The new authority replays with the FULL original flags and original evidence.
    integrator(
        &f,
        "authority",
        &["--mode", "dispatcher", "--all-participants-upgraded"],
    );
    integrator(&f, "publish", &["--base", &base]);
    f.run(&[&["register"][..], &dispatcher_args("dispatch-one", &[])].concat());
    succeeded(&routed(
        &f,
        &ticket,
        &base,
        full(("dispatcher", "dispatch-one")),
    ));
    // A different dispatcher identity replaces it and replays the same durable reservation.
    f.run(&[
        "leave",
        "--role",
        "dispatcher",
        "--id",
        "dispatcher",
        "--session",
        "dispatch-one",
    ]);
    register_with_lease(&f, "dispatcher", "relief", "relief-one", "600");
    succeeded(&routed(&f, &ticket, &base, full(("relief", "relief-one"))));
    // The replaced generation and identity remain fenced.
    refused(
        &routed(&f, &ticket, &base, full(("dispatcher", "dispatch-one"))),
        "exact dispatcher session",
    );
    let state = f.run(&["status"]).state;
    assert_eq!(state.assignments.len(), 1);
    assert_eq!(
        state.assignments["op-legacy"].return_route,
        ReturnRoute::Terminal("worker-tty".into())
    );

    // Conflicting routes and evidence for a different operation stay refused.
    let human = route_evidence(&f, &ticket, "op-legacy", first, "human", "relief");
    refused(
        &routed(
            &f,
            &ticket,
            &base,
            Reservation {
                route: Some(("human", &human)),
                ..full(("relief", "relief-one"))
            },
        ),
        "different assignment",
    );
    // Authored by the current authority, so only the operation mismatch can refuse it.
    let misaddressed = route_evidence(&f, &ticket, "op-legacy", first, tty, "relief");
    refused(
        &routed(
            &f,
            &ticket,
            &base,
            Reservation {
                operation: "op-other",
                route: Some((tty, &misaddressed)),
                ..full(("relief", "relief-one"))
            },
        ),
        "exact operation, worker, generation and route",
    );
    // A NEW operation still needs evidence authored by the current authority.
    register_with_lease(&f, "worker", "second", "second-session", "600");
    let by_root = route_evidence(
        &f,
        &other,
        "op-new",
        ("second", "second-session"),
        tty,
        "integrator",
    );
    refused(
        &routed(
            &f,
            &other,
            &base,
            Reservation {
                authority: ("relief", "relief-one"),
                operation: "op-new",
                worker: ("second", "second-session"),
                scope: Some("b"),
                route: Some((tty, &by_root)),
            },
        ),
        "exact active assignment authority",
    );
}
