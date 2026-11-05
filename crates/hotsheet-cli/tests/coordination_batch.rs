//! Real separate-process batch integration (HS2-9G0M3V): collect, open, push once, CI
//! result, follow-ups, circuit breaker and recovery. Every pool here is a disposable,
//! independently initialized project, remote and ticket store.

#[allow(dead_code)] // HS2-9G0M3V: shared process fixture also provides unused waiter helpers.
#[path = "support/coordination.rs"]
mod support;

use std::fs;

use hotsheet_ticketing::git_connection_id;
use support::{Fixture, git};

fn output(f: &Fixture, args: &[&str]) -> std::process::Output {
    let args = f.coord(args);
    f.command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap()
}

fn json(f: &Fixture, args: &[&str]) -> serde_json::Value {
    let out = output(f, args);
    assert!(
        out.status.success(),
        "CLI failed for {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn refused(f: &Fixture, args: &[&str], reason: &str) {
    let out = output(f, args);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "expected refusal for {args:?}");
    assert!(stderr.contains(reason), "{stderr}");
}

const ID: [&str; 4] = ["--id", "integrator", "--session", "integrator-session"];

fn with_id<'a>(command: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    [&[command][..], &ID[..], extra].concat()
}

/// A reviewed code handoff by its own worker: a commit on a worker branch that changes
/// `path`, assigned, claimed, released and handed off.
fn handoff(
    f: &Fixture,
    worker: &str,
    operation: &str,
    path: &str,
) -> (hotsheet_model::Ticket, String) {
    let ticket = f.ticket();
    let base = git(&f.project, &["rev-parse", "main"]);
    let worktree = f.root.path().join(format!("worktree-{worker}"));
    git(
        &f.project,
        &[
            "worktree",
            "add",
            "-qb",
            &format!("work-{worker}"),
            worktree.to_str().unwrap(),
            &base,
        ],
    );
    fs::write(worktree.join(path), format!("{operation}\n")).unwrap();
    git(&worktree, &["add", path]);
    git(
        &worktree,
        &[
            "commit",
            "-qm",
            &format!("{} synthetic change", ticket.slug),
        ],
    );
    let commit = git(&worktree, &["rev-parse", "HEAD"]);
    json(
        f,
        &[
            "register",
            "--role",
            "worker",
            "--id",
            worker,
            "--session",
            "worker-session",
        ],
    );
    json(
        f,
        &with_id(
            "assign",
            &[
                "--worker",
                worker,
                "--worker-session",
                "worker-session",
                "--operation",
                operation,
                "--connection",
                &git_connection_id(&f.store),
                "--native-id",
                &ticket.id.to_string(),
                "--slug",
                &ticket.slug,
                "--base",
                &base,
            ],
        ),
    );
    f.cli(&["claim", &ticket.slug, "--worker", worker, "--eta", "10m"]);
    let worker_args = [
        "--id",
        worker,
        "--session",
        "worker-session",
        "--assignment",
        operation,
    ];
    json(
        f,
        &[&["worker"][..], &worker_args, &["--phase", "accepted"]].concat(),
    );
    f.cli(&["release", &ticket.slug, "--worker", worker]);
    f.cli(&[
        "edit",
        &ticket.slug,
        "--note",
        &format!("## Integration handoff\nVerified synthetic commit {commit}"),
    ]);
    json(
        f,
        &[
            &["worker"][..],
            &worker_args,
            &["--phase", "handoff", "--commit", &commit],
        ]
        .concat(),
    );
    (ticket, commit)
}

fn batch_fixture(max_red_batches: &str) -> Fixture {
    let f = Fixture::new();
    let remote = f.root.path().join("remote.git");
    git(
        &f.project,
        &["init", "-q", "--bare", remote.to_str().unwrap()],
    );
    git(
        &f.project,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&f.project, &["push", "-q", "-u", "origin", "main"]);
    let report = json(
        &f,
        &[
            "strategy-allow",
            "--strategy",
            "batch",
            "--reason",
            "maintainer-approved throughput",
            "--max-red-batches",
            max_red_batches,
        ],
    );
    assert_eq!(
        report["policy"]["allowed"][0]["circuit_breaker"],
        serde_json::json!({"max_red_batches": max_red_batches.parse::<u32>().unwrap(), "max_red_minutes": 120})
    );
    json(
        &f,
        &with_id(
            "register",
            &[
                "--role",
                "integrator",
                "--strategy",
                "batch",
                "--all-participants-upgraded",
                "--lease-seconds",
                "3600",
            ],
        ),
    );
    f
}

#[test]
fn hs2_9g0m3v_batch_flow_collects_pushes_once_and_gates_red_completion_on_follow_ups() {
    let f = batch_fixture("1");
    let (first, first_sha) = handoff(&f, "worker-1", "op-1", "first.txt");
    let (second, second_sha) = handoff(&f, "worker-2", "op-2", "second.txt");
    let (_third, _) = handoff(&f, "worker-3", "op-3", "first.txt");
    // The second ticket is more urgent, so it integrates first.
    f.cli(&["edit", &second.slug, "--priority", "high"]);
    let plan = json(&f, &["batch-plan"]);
    let selected: Vec<&str> = plan["selected"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["assignment"].as_str().unwrap())
        .collect();
    assert_eq!(selected, ["op-2", "op-1"]);
    assert_eq!(plan["deferred"][0]["assignment"], "op-3");
    assert!(
        plan["deferred"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("overlap")
    );
    // Opening the conflicting pair is refused; the planned pair opens atomically.
    refused(
        &f,
        &with_id(
            "batch-open",
            &[
                "--batch",
                "batch-1",
                "--assignment",
                "op-1",
                "--assignment",
                "op-3",
            ],
        ),
        "overlapping paths",
    );
    let opened = json(
        &f,
        &with_id(
            "batch-open",
            &[
                "--batch",
                "batch-1",
                "--assignment",
                "op-2",
                "--assignment",
                "op-1",
            ],
        ),
    );
    assert_eq!(opened["state"], "integrating");
    assert_eq!(opened["tickets"][1]["source_commit"], first_sha.as_str());
    assert_eq!(
        opened["tickets"][1]["paths"],
        serde_json::json!(["first.txt"])
    );
    // Integrate both onto main in batch order and push once.
    git(&f.project, &["cherry-pick", &second_sha]);
    git(&f.project, &["cherry-pick", &first_sha]);
    let main = git(&f.project, &["rev-parse", "HEAD"]);
    let second_shipping = git(&f.project, &["rev-parse", "HEAD~1"]);
    let push = [
        "--batch",
        "batch-1",
        "--main-commit",
        &main,
        "--shipping",
        &format!("op-1={main}"),
        "--shipping",
        &format!("op-2={second_shipping}"),
        "--smoke-check",
        "cargo lint",
    ];
    refused(&f, &with_id("batch-pushed", &push), "upstream");
    git(&f.project, &["push", "-q", "origin", "main"]);
    refused(
        &f,
        &with_id(
            "batch-pushed",
            &[
                "--batch",
                "batch-1",
                "--main-commit",
                &main,
                "--shipping",
                &format!("op-1={first_sha}"),
                "--shipping",
                &format!("op-2={second_shipping}"),
                "--smoke-check",
                "cargo lint",
            ],
        ),
        "not contained in the batch main commit",
    );
    let pushed = json(&f, &with_id("batch-pushed", &push));
    assert_eq!(pushed["state"], "ci");
    assert_eq!(pushed["main_commit"], main.as_str());
    let status = f.run(&["status"]);
    assert_eq!(
        status.state.assignments["op-1"].phase,
        hotsheet_cli::coordination::Phase::Ci
    );
    let batches = status.batches.unwrap();
    assert_eq!(batches.active[0].id, "batch-1");
    // Red CI trips the one-batch breaker and names the fallback.
    let red = json(
        &f,
        &with_id(
            "batch-result",
            &[
                "--batch",
                "batch-1",
                "--run",
                "77",
                "--conclusion",
                "failure",
            ],
        ),
    );
    assert!(
        red["notify"]
            .as_str()
            .unwrap()
            .contains("fell back to serial")
    );
    refused(
        &f,
        &with_id("mark", &["--assignment", "op-1", "--phase", "done"]),
        "record a follow-up for each failure",
    );
    // Follow-ups must exist in the store, then unblock completion.
    let attribution = f.root.path().join("attribution.json");
    fs::write(
        &attribution,
        r#"{"filed":[{"key":"k-missing","kind":"failure","slug":"HS-999","links":[]}]}"#,
    )
    .unwrap();
    refused(
        &f,
        &with_id(
            "batch-follow-ups",
            &[
                "--batch",
                "batch-1",
                "--attribution-file",
                attribution.to_str().unwrap(),
            ],
        ),
        "not in the selected ticket store",
    );
    let bug = f.ticket();
    fs::write(
        &attribution,
        format!(
            r#"{{"filed":[{{"key":"k-1","kind":"failure","slug":"{}","links":["{}"]}},{{"key":"k-flaky","kind":"flaky","slug":"{}","links":[]}}]}}"#,
            bug.slug, first.slug, bug.slug
        ),
    )
    .unwrap();
    let recorded = json(
        &f,
        &with_id(
            "batch-follow-ups",
            &[
                "--batch",
                "batch-1",
                "--attribution-file",
                attribution.to_str().unwrap(),
            ],
        ),
    );
    assert_eq!(recorded["failures"].as_array().unwrap().len(), 1);
    assert_eq!(
        recorded["failures"][0]["follow_up_ticket"]["native_id"],
        bug.id.to_string().as_str()
    );
    for operation in ["op-1", "op-2"] {
        f.run(&with_id(
            "mark",
            &["--assignment", operation, "--phase", "done"],
        ));
    }
    // The tripped breaker refuses the next batch and reports serial in status.
    refused(
        &f,
        &with_id(
            "batch-open",
            &["--batch", "batch-2", "--assignment", "op-3"],
        ),
        "circuit breaker is tripped",
    );
    let status = f.run(&["status"]);
    assert_eq!(
        status.batches.unwrap().effective_strategy,
        hotsheet_cli::coordination::IntegrationStrategy::Serial
    );
    // A green run must contain the last red push before the breaker clears.
    refused(
        &f,
        &with_id(
            "batch-recover",
            &[
                "--run",
                "78",
                "--main-commit",
                &git(&f.project, &["rev-parse", "HEAD~2"]),
            ],
        ),
        "does not contain the last red batch push",
    );
    let recovered = json(
        &f,
        &with_id("batch-recover", &["--run", "78", "--main-commit", &main]),
    );
    assert!(recovered.get("tripped").is_none());
    let ledger = json(&f, &["batch-status"]);
    assert_eq!(ledger["version"], 1);
    assert_eq!(ledger["batches"]["batch-1"]["state"], "red");
    assert_eq!(
        ledger["batches"]["batch-1"]["smoke_checks"],
        serde_json::json!(["cargo lint"])
    );
    let opened = json(
        &f,
        &with_id(
            "batch-open",
            &["--batch", "batch-2", "--assignment", "op-3"],
        ),
    );
    assert_eq!(opened["state"], "integrating");
}

#[test]
fn hs2_9g0m3v_smoke_failure_abort_and_serial_sessions_never_batch() {
    let f = batch_fixture("2");
    handoff(&f, "worker-1", "op-1", "first.txt");
    json(
        &f,
        &with_id(
            "batch-open",
            &["--batch", "batch-1", "--assignment", "op-1"],
        ),
    );
    let aborted = json(
        &f,
        &with_id(
            "batch-abort",
            &[
                "--batch",
                "batch-1",
                "--reason",
                "smoke gate failed: cargo lint",
            ],
        ),
    );
    assert_eq!(aborted["state"], "aborted");
    assert!(aborted.get("main_commit").is_none());
    let status = f.run(&["status"]);
    assert_eq!(
        status.state.assignments["op-1"].phase,
        hotsheet_cli::coordination::Phase::Handoff
    );
    // A serial generation of the same integrator cannot open a batch.
    json(
        &f,
        &[
            "leave",
            "--role",
            "integrator",
            "--id",
            "integrator",
            "--session",
            "integrator-session",
        ],
    );
    json(
        &f,
        &[
            "register",
            "--role",
            "integrator",
            "--id",
            "integrator",
            "--session",
            "serial-session",
            "--strategy",
            "serial",
        ],
    );
    refused(
        &f,
        &[
            "batch-open",
            "--id",
            "integrator",
            "--session",
            "serial-session",
            "--batch",
            "batch-2",
            "--assignment",
            "op-1",
        ],
        "this session runs serial",
    );
    let capabilities = json(&f, &["capabilities"]);
    assert_eq!(capabilities["integration_batches"], 1);
    let strategies = json(&f, &["strategies"]);
    let batch = strategies["registry"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "batch")
        .unwrap()
        .clone();
    assert_eq!(batch["implemented"], true);
}
