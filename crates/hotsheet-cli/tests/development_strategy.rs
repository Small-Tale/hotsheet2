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
