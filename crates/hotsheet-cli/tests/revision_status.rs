use assert_cmd::Command;
use serde_json::Value;

#[test]
fn cli_and_migrator_report_the_same_current_source_revision_without_a_store() {
    let statuses: Vec<Value> = ["hotsheet-cli", "hotsheet-migrate"]
        .into_iter()
        .map(|name| {
            let output = Command::cargo_bin(name)
                .unwrap()
                .arg("--revision-status")
                .current_dir(std::env::temp_dir())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice(&output.stdout).unwrap()
        })
        .collect();
    assert_eq!(statuses[0], statuses[1]);
    let revision = statuses[0]["build_revision"].as_str().unwrap();
    assert!(revision.starts_with("source-sha256:"));
    assert_eq!(revision, statuses[0]["source_revision"]);
    assert_eq!(statuses[0]["source_stale"], false);
}
