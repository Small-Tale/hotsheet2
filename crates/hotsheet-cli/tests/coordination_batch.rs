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
    // HS2-GT57BQ: ledger version two adds staged batches.
    assert_eq!(capabilities["integration_batches"], 2);
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

/// A ci-staged pool (HS2-GT57BQ): a bare remote, the strategy allowed with an explicit
/// staging branch, and a live ci-staged integrator.
fn staged_fixture() -> Fixture {
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
            "ci-staged",
            "--reason",
            "keep main green",
            "--staging-branch",
            "integration",
        ],
    );
    assert_eq!(
        report["policy"]["allowed"][0]["staging_branch"],
        "integration"
    );
    refused(
        &f,
        &[
            "strategy-allow",
            "--strategy",
            "ci-staged",
            "--reason",
            "x",
            "--staging-branch",
            "main",
        ],
        "must not be main",
    );
    json(
        &f,
        &with_id(
            "register",
            &[
                "--role",
                "integrator",
                "--strategy",
                "ci-staged",
                "--all-participants-upgraded",
                "--lease-seconds",
                "3600",
            ],
        ),
    );
    f
}

fn stage_args<'a>(batch: &'a str, staged: &'a str, shipping: &'a str) -> Vec<&'a str> {
    vec![
        "--batch",
        batch,
        "--staged-commit",
        staged,
        "--shipping",
        shipping,
        "--smoke-check",
        "cargo lint",
    ]
}

#[test]
fn hs2_gt57bq_staged_flow_stacks_promotes_exact_green_sha_and_keeps_main_on_red() {
    let f = staged_fixture();
    let main = git(&f.project, &["rev-parse", "main"]);
    let (_first, first_sha) = handoff(&f, "worker-1", "op-1", "first.txt");
    let (_second, second_sha) = handoff(&f, "worker-2", "op-2", "second.txt");
    let (third, third_sha) = handoff(&f, "worker-3", "op-3", "third.txt");
    let opened = json(
        &f,
        &with_id(
            "batch-open",
            &["--batch", "batch-1", "--assignment", "op-1"],
        ),
    );
    assert_eq!(opened["staging"]["branch"], "integration");
    assert_eq!(opened["staging"]["base_commit"], main.as_str());
    // Build the integration branch from origin/main and stage it.
    git(&f.project, &["switch", "-qc", "integration", "origin/main"]);
    git(&f.project, &["cherry-pick", &first_sha]);
    let staged_1 = git(&f.project, &["rev-parse", "HEAD"]);
    let shipping_1 = format!("op-1={staged_1}");
    refused(
        &f,
        &with_id(
            "batch-staged",
            &stage_args("batch-1", &staged_1, &shipping_1),
        ),
        "fetch origin first",
    );
    git(&f.project, &["push", "-q", "origin", "integration"]);
    // A staged batch never records a main push.
    refused(
        &f,
        &with_id(
            "batch-pushed",
            &[
                "--batch",
                "batch-1",
                "--main-commit",
                &staged_1,
                "--shipping",
                &shipping_1,
                "--smoke-check",
                "cargo lint",
            ],
        ),
        "",
    );
    let staged = json(
        &f,
        &with_id(
            "batch-staged",
            &stage_args("batch-1", &staged_1, &shipping_1),
        ),
    );
    assert_eq!(staged["state"], "ci");
    assert_eq!(staged["staging"]["staged_commit"], staged_1.as_str());
    assert_eq!(git(&f.project, &["rev-parse", "origin/main"]), main);
    // Stack the next batch on the staged tip while the first is in CI.
    let stacked = json(
        &f,
        &with_id(
            "batch-open",
            &["--batch", "batch-2", "--assignment", "op-2"],
        ),
    );
    assert_eq!(stacked["staging"]["parent"], "batch-1");
    assert_eq!(stacked["staging"]["base_commit"], staged_1.as_str());
    git(&f.project, &["cherry-pick", &second_sha]);
    let staged_2 = git(&f.project, &["rev-parse", "HEAD"]);
    git(&f.project, &["push", "-q", "origin", "integration"]);
    let shipping_2 = format!("op-2={staged_2}");
    json(
        &f,
        &with_id(
            "batch-staged",
            &stage_args("batch-2", &staged_2, &shipping_2),
        ),
    );
    let ledger = json(&f, &["batch-status"]);
    assert_eq!(ledger["version"], 2);
    assert_eq!(
        ledger["staging"]["in_ci"],
        serde_json::json!(["batch-1", "batch-2"])
    );
    assert_eq!(ledger["staging"]["tip"], staged_2.as_str());
    assert_eq!(
        ledger["staging"]["waiting_tickets"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let snapshot = f.run(&["status"]);
    assert_eq!(
        snapshot
            .batches
            .unwrap()
            .staging
            .unwrap()
            .tip_batch
            .as_deref(),
        Some("batch-2")
    );
    // Green on the tip implies its parent; promotion needs main to equal the exact SHA.
    let green = json(
        &f,
        &with_id(
            "batch-result",
            &[
                "--batch",
                "batch-2",
                "--run",
                "91",
                "--conclusion",
                "success",
            ],
        ),
    );
    assert_eq!(green["batch"]["state"], "green");
    let promote = ["--batch", "batch-2", "--main-commit", &staged_2];
    refused(&f, &with_id("batch-promote", &promote), "push.sh --promote");
    refused(
        &f,
        &with_id("mark", &["--assignment", "op-1", "--phase", "done"]),
        "promoted",
    );
    git(
        &f.project,
        &[
            "push",
            "-q",
            "origin",
            &format!("{staged_2}:refs/heads/main"),
        ],
    );
    let promoted = json(&f, &with_id("batch-promote", &promote));
    assert_eq!(promoted["state"], "promoted");
    assert_eq!(promoted["main_commit"], staged_2.as_str());
    let ledger = json(&f, &["batch-status"]);
    assert_eq!(ledger["batches"]["batch-1"]["state"], "promoted");
    assert_eq!(
        ledger["batches"]["batch-1"]["staging"]["implied_by"],
        "batch-2"
    );
    for operation in ["op-1", "op-2"] {
        f.run(&with_id(
            "mark",
            &["--assignment", operation, "--phase", "done"],
        ));
    }
    // Red: main stays at the promoted SHA; the follow-up rule gates re-staging.
    json(
        &f,
        &with_id(
            "batch-open",
            &["--batch", "batch-3", "--assignment", "op-3"],
        ),
    );
    git(&f.project, &["cherry-pick", &third_sha]);
    let staged_3 = git(&f.project, &["rev-parse", "HEAD"]);
    git(&f.project, &["push", "-q", "origin", "integration"]);
    let shipping_3 = format!("op-3={staged_3}");
    json(
        &f,
        &with_id(
            "batch-staged",
            &stage_args("batch-3", &staged_3, &shipping_3),
        ),
    );
    let red = json(
        &f,
        &with_id(
            "batch-result",
            &[
                "--batch",
                "batch-3",
                "--run",
                "92",
                "--conclusion",
                "failure",
            ],
        ),
    );
    assert_eq!(red["batch"]["state"], "red");
    assert!(red["breaker"].get("last_red_main").is_none());
    git(&f.project, &["fetch", "-q", "origin"]);
    assert_eq!(git(&f.project, &["rev-parse", "origin/main"]), staged_2);
    let restage = [
        "--batch",
        "batch-3",
        "--reason",
        "red run 92",
        "--hold",
        "op-3",
    ];
    refused(
        &f,
        &with_id("batch-restage", &restage),
        "follow-up for each failure",
    );
    let bug = f.ticket();
    json(
        &f,
        &with_id(
            "batch-follow-ups",
            &[
                "--batch",
                "batch-3",
                "--failure",
                &format!("k-92={}", bug.slug),
            ],
        ),
    );
    let restaged = json(&f, &with_id("batch-restage", &restage));
    assert_eq!(restaged["state"], "restaged");
    let plan = json(&f, &["batch-plan"]);
    assert_eq!(plan["deferred"][0]["slug"], third.slug.as_str());
    assert!(
        plan["deferred"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("held as a suspect")
    );
    refused(
        &f,
        &with_id(
            "batch-open",
            &["--batch", "batch-4", "--assignment", "op-3"],
        ),
        "held as a suspect",
    );
}

#[test]
fn hs2_0bj1a3_headless_status_uses_shared_goal_without_opening_a_batch() {
    let f = staged_fixture();
    json(
        &f,
        &[
            "development-strategy-set",
            "--preset",
            "steady-main",
            "--green-main-goal-minutes",
            "90",
        ],
    );
    let status = json(&f, &["batch-status"]);
    assert_eq!(status["cadence"]["goal_minutes"], 90);
    assert_eq!(
        status["cadence"]["last_green_main_at"],
        serde_json::Value::Null
    );
    assert_eq!(status["cadence"]["action"], "no_ready_work");
    assert!(status["batches"].as_object().unwrap().is_empty());
}
