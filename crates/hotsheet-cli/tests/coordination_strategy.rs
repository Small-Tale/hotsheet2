//! Real separate-process integration strategy policy and registration (HS2-NEY1QH).
//! Every pool here is a disposable, independently initialized project and ticket store.

#[path = "support/coordination.rs"]
mod support;

use hotsheet_cli::coordination::IntegrationStrategy;
use support::Fixture;

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
        "CLI failed: {}",
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

const INTEGRATOR: [&str; 7] = [
    "register",
    "--role",
    "integrator",
    "--id",
    "integrator",
    "--session",
    "integrator-session",
];

#[test]
fn hs2_ney1qh_policy_commands_never_open_the_pool_and_validate_reasons() {
    let f = Fixture::new();
    let report = json(&f, &["strategies"]);
    assert_eq!(report["policy"]["default"], "serial");
    assert_eq!(report["policy"]["allowed"], serde_json::json!([]));
    let implemented: Vec<_> = report["registry"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["implemented"] == true)
        .map(|d| d["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(implemented, ["serial", "batch", "ci-staged"]);
    refused(
        &f,
        &["strategy-allow", "--strategy", "batch", "--reason", " "],
        "requires a reason",
    );
    refused(
        &f,
        &["strategy-default", "--strategy", "batch"],
        "must be serial or an allowed strategy",
    );
    let allowed = json(
        &f,
        &[
            "strategy-allow",
            "--strategy",
            "batch",
            "--reason",
            "maintainer-approved throughput",
        ],
    );
    assert_eq!(
        allowed["policy"]["allowed"],
        serde_json::json!([{"strategy": "batch", "reason": "maintainer-approved throughput"}])
    );
    // The policy is committed shared project settings, not coordination state.
    let shared = std::fs::read_to_string(f.project.join(".hotsheet2/settings.json")).unwrap();
    assert!(shared.contains("integration_strategies"), "{shared}");
    assert!(!f.project.join(".git/hotsheet-coordination").exists());
    json(&f, &["strategy-default", "--strategy", "batch"]);
    refused(
        &f,
        &["strategy-disallow", "--strategy", "batch"],
        "set another default first",
    );
    json(&f, &["strategy-default", "--strategy", "serial"]);
    let withdrawn = json(&f, &["strategy-disallow", "--strategy", "batch"]);
    assert_eq!(withdrawn["policy"]["allowed"], serde_json::json!([]));
}

#[test]
fn hs2_ney1qh_register_refuses_disallowed_and_unimplemented_strategies_and_reports_serial() {
    let f = Fixture::new();
    refused(
        &f,
        &[&INTEGRATOR[..], &["--strategy", "batch"]].concat(),
        "not allowed by this project",
    );
    refused(
        &f,
        &[&INTEGRATOR[..], &["--strategy", "merge-queue"]].concat(),
        "invalid value",
    );
    json(
        &f,
        &[
            "strategy-allow",
            "--strategy",
            "ci-staged",
            "--reason",
            "keep main green",
        ],
    );
    // HS2-GT57BQ: ci-staged is implemented; an allowed one still needs the upgrade flag.
    refused(
        &f,
        &[&INTEGRATOR[..], &["--strategy", "ci-staged"]].concat(),
        "all-participants-upgraded",
    );
    refused(
        &f,
        &[
            "register",
            "--role",
            "worker",
            "--id",
            "worker",
            "--session",
            "worker-session",
            "--strategy",
            "serial",
        ],
        "only the integrator selects an integration strategy",
    );
    // Every refusal above left the pool on its legacy schema without a session.
    let status = f.run(&["status"]);
    assert_eq!(status.state.version, 3);
    assert!(status.state.integrator.is_none());
    assert_eq!(status.integration_strategy, None);

    let registered = f.run(&[&INTEGRATOR[..], &["--strategy", "serial"]].concat());
    assert_eq!(registered.state.version, 3);
    assert_eq!(
        registered.integration_strategy,
        Some(IntegrationStrategy::Serial)
    );
    let raw =
        std::fs::read_to_string(f.project.join(".git/hotsheet-coordination/state.json")).unwrap();
    assert!(!raw.contains("strategy"), "{raw}");
    let status = json(&f, &["status"]);
    assert_eq!(status["integration_strategy"], "serial");
    let workers = json(&f, &["workers"]);
    assert_eq!(workers["integration_strategy"], "serial");
    assert_eq!(workers["workers"][0]["role"], "integrator");
    assert_eq!(workers["workers"][0]["integration_strategy"], "serial");
}

#[test]
fn hs2_ney1qh_invalid_committed_policy_is_reported_not_ignored() {
    let f = Fixture::new();
    // A disposable project, never the developer's checkout.
    assert!(f.project.starts_with(f.root.path()));
    std::fs::create_dir_all(f.project.join(".hotsheet2")).unwrap();
    std::fs::write(
        f.project.join(".hotsheet2/settings.json"),
        r#"{"$hotsheetSchema":1,"integration_strategies":{"default":"batch"}}"#,
    )
    .unwrap();
    refused(&f, &INTEGRATOR, "must be serial or an allowed strategy");
    refused(&f, &["strategies"], "must be serial or an allowed strategy");
}

#[test]
fn hs2_ney1qh_serial_integrator_next_and_a_waiting_worker_keep_todays_dispatch() {
    let f = Fixture::new();
    let ticket = f.ticket();
    let mut worker = f.waiting(&[
        "next",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--lease-seconds",
        "600",
    ]);
    // The integrator's own wait records the strategy on its first registration.
    let integrator = f.run(&[
        "next",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "integrator-session",
        "--lease-seconds",
        "600",
        "--strategy",
        "serial",
    ]);
    assert_eq!(
        integrator.integration_strategy,
        Some(IntegrationStrategy::Serial)
    );
    let base = support::git(&f.project, &["rev-parse", "HEAD"]);
    f.assign("op-serial", &ticket, &base, "integrator-session");
    let assigned = worker.result();
    assert!(assigned.state.assignments.contains_key("op-serial"));
    assert_eq!(assigned.state.version, 3);
    assert_eq!(
        assigned.integration_strategy,
        Some(IntegrationStrategy::Serial)
    );
}

/// HS2-6A1MEQ: the breaker thresholds set the batch and the ci-staged breaker, each recorded
/// on its own strategy entry, so the help must not call them batch-only.
#[test]
fn hs2_6a1meq_strategy_allow_help_names_both_breaker_strategies() {
    let f = Fixture::new();
    let out = output(&f, &["strategy-allow", "--help"]);
    assert!(out.status.success());
    let help = String::from_utf8_lossy(&out.stdout);
    for flag in ["--max-red-batches", "--max-red-minutes"] {
        let line = help
            .lines()
            .skip_while(|line| !line.contains(flag))
            .nth(1)
            .unwrap_or_else(|| panic!("{flag} has no description in:\n{help}"));
        assert!(line.contains("Batch and ci-staged"), "{flag}: {line}");
    }
    assert!(!help.contains("Batch only"), "{help}");
    assert!(
        help.contains("ci-staged only"),
        "the staging branch stays ci-staged only"
    );
}
