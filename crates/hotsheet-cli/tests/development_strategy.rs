//! Headless project policy commands (HS2-BF79YX).

use std::process::Command;

use hotsheet_ticketing::Settings;

fn run(project: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_hotsheet-cli"))
        .arg("coordination")
        .arg("--project")
        .arg(project)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn hs2_bf79yx_cli_reads_validates_updates_and_resolves_shared_policy() {
    let project = tempfile::tempdir().unwrap();
    let project = project.path();
    let default = run(project, &["development-strategies"]);
    assert!(
        default.status.success(),
        "{}",
        String::from_utf8_lossy(&default.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&default.stdout).unwrap();
    assert_eq!(value["effective"]["preset"], "ticket-assured");
    assert_eq!(value["effective"]["integration"], "serial");
    assert_eq!(value["effective"]["source"], "unconfigured-project");
    assert_eq!(value["effective"]["local_gate"], "risk-selected");
    let refused = run(
        project,
        &["development-strategy-set", "--preset", "steady-main"],
    );
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr)
            .contains("conflicts with project integration policy")
    );
    let settings = Settings::for_project(project);
    assert_eq!(
        hotsheet_cli::coordination::DevelopmentPolicy::read(&settings).unwrap(),
        Default::default()
    );
    let allowed = run(
        project,
        &[
            "strategy-allow",
            "--strategy",
            "ci-staged",
            "--reason",
            "synthetic staging",
        ],
    );
    assert!(
        allowed.status.success(),
        "{}",
        String::from_utf8_lossy(&allowed.stderr)
    );
    let batch = run(
        project,
        &[
            "strategy-allow",
            "--strategy",
            "batch",
            "--reason",
            "reviewed existing batch policy",
        ],
    );
    assert!(
        batch.status.success(),
        "{}",
        String::from_utf8_lossy(&batch.stderr)
    );
    let batch_default = run(project, &["strategy-default", "--strategy", "batch"]);
    assert!(
        batch_default.status.success(),
        "{}",
        String::from_utf8_lossy(&batch_default.stderr)
    );
    let migrated = run(project, &["development-strategies"]);
    assert!(migrated.status.success());
    let migrated: serde_json::Value = serde_json::from_slice(&migrated.stdout).unwrap();
    assert_eq!(migrated["effective"]["source"], "unconfigured-project");
    assert_eq!(migrated["effective"]["integration"], "batch");
    assert_eq!(migrated["effective"]["local_gate"], "risk-selected");
    for preset in ["steady-main", "risk-guided", "ticket-assured"] {
        let saved = run(
            project,
            &[
                "development-strategy-set",
                "--preset",
                preset,
                "--green-main-goal-minutes",
                "420",
            ],
        );
        assert!(
            saved.status.success(),
            "{}",
            String::from_utf8_lossy(&saved.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&saved.stdout).unwrap();
        assert_eq!(value["effective"]["preset"], preset);
        assert_eq!(value["policy"]["green_main_goal_minutes"], 420);
        if preset == "ticket-assured" {
            assert_eq!(value["effective"]["integration"], "serial");
            assert_eq!(value["effective"]["local_gate"], "full-local");
        }
    }
    let resolved = run(
        project,
        &[
            "development-strategy-resolve",
            "--session",
            "steady-main",
            "--ticket",
            "risk-guided",
        ],
    );
    assert!(
        resolved.status.success(),
        "{}",
        String::from_utf8_lossy(&resolved.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&resolved.stdout).unwrap();
    assert_eq!(value["preset"], "risk-guided");
    assert_eq!(value["source"], "ticket");
    assert_eq!(value["integration"], "ci-staged");
    assert_eq!(value["require_green_main_sha"], true);
}

#[test]
fn hs2_arjnjv_headless_custom_policy_round_trip_precedence_and_rollback() {
    let project = tempfile::tempdir().unwrap();
    let project = project.path();
    let definition = project.join("strict.json");
    std::fs::write(
        &definition,
        r#"{"integration":"serial","local_gate":"full-local","minimum_tier":"T2","browser":true,"platform":true,"test_plan_timing":"before-work","ci_gate":"CI","red_ci_recovery":"attribute-and-retry"}"#,
    )
    .unwrap();
    let args = [
        "development-custom-set",
        "--name",
        "strict-review",
        "--file",
        definition.to_str().unwrap(),
        "--select-default",
    ];
    let missing_ci = run(project, &args);
    assert!(!missing_ci.status.success());
    assert!(String::from_utf8_lossy(&missing_ci.stderr).contains("ci.yml CI workflow"));
    std::fs::create_dir_all(project.join(".github/workflows")).unwrap();
    std::fs::write(
        project.join(".github/workflows/ci.yml"),
        "name: CI\non: push\n",
    )
    .unwrap();
    let missing_remote = run(project, &args);
    assert!(!missing_remote.status.success());
    assert!(String::from_utf8_lossy(&missing_remote.stderr).contains("origin remote"));
    assert!(
        hotsheet_ticketing::git::launch("git")
            .args(["init", "-q"])
            .arg(project)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        hotsheet_ticketing::git::launch("git")
            .args([
                "-C",
                project.to_str().unwrap(),
                "remote",
                "add",
                "origin",
                "https://example.invalid/synthetic.git"
            ])
            .status()
            .unwrap()
            .success()
    );
    let set = run(project, &args);
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&set.stdout).unwrap();
    assert_eq!(report["policy"]["version"], 2);
    assert_eq!(report["effective"]["preset"], "strict-review");
    assert_eq!(report["effective"]["source"], "project");
    assert_eq!(report["effective"]["custom"]["minimum_tier"], "T2");
    let allowed = run(
        project,
        &[
            "strategy-allow",
            "--strategy",
            "ci-staged",
            "--reason",
            "synthetic staged policy",
        ],
    );
    assert!(
        allowed.status.success(),
        "{}",
        String::from_utf8_lossy(&allowed.stderr)
    );
    let staged_file = project.join("staged.json");
    std::fs::write(&staged_file,
        r#"{"integration":"ci-staged","local_gate":"targeted","minimum_tier":"T1","budget_minutes":5,"test_plan_timing":"before-handoff","ci_gate":"CI","staging_branch":"integration","red_ci_recovery":"fallback-to-serial"}"#,
    ).unwrap();
    let staged = run(
        project,
        &[
            "development-custom-set",
            "--name",
            "fast-staged",
            "--file",
            staged_file.to_str().unwrap(),
        ],
    );
    assert!(
        staged.status.success(),
        "scratch integration branch need not exist: {}",
        String::from_utf8_lossy(&staged.stderr)
    );
    let overridden = run(
        project,
        &[
            "development-strategy-resolve",
            "--session",
            "strict-review",
            "--ticket",
            "ticket-assured",
        ],
    );
    assert!(
        overridden.status.success(),
        "{}",
        String::from_utf8_lossy(&overridden.stderr)
    );
    let resolved: serde_json::Value = serde_json::from_slice(&overridden.stdout).unwrap();
    assert_eq!(resolved["preset"], "ticket-assured");
    assert_eq!(resolved["source"], "ticket");
    let restored = run(
        project,
        &["development-strategy-set", "--preset", "ticket-assured"],
    );
    assert!(
        restored.status.success(),
        "{}",
        String::from_utf8_lossy(&restored.stderr)
    );
    let restored: serde_json::Value = serde_json::from_slice(&restored.stdout).unwrap();
    assert_eq!(
        restored["policy"]["default_custom"],
        serde_json::Value::Null
    );
    assert_eq!(restored["effective"]["preset"], "ticket-assured");
    assert_eq!(
        restored["policy"]["custom"]["strict-review"]["ci_gate"],
        "CI"
    );
}
