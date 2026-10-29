//! HS2-R5HTZN headless E2E: `hotsheet-cli notify` sets up an external notifier plugin
//! channel with a stored credential and rules, sends through the rules, tests the channel,
//! and removes it, with a real plugin subprocess speaking the stdin/stdout protocol.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::str::contains;
use serde_json::Value;

struct Env {
    _root: tempfile::TempDir,
    home: PathBuf,
    outbox: PathBuf,
}

/// A plugin that appends each envelope to `outbox` and answers like a real one: it fails
/// permanently when its `fail` setting is "yes".
fn install_plugin(home: &Path, outbox: &Path) {
    let dir = home.join("notifiers").join("recorder");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        r#"id = "recorder"
display_name = "Recorder"
description = "Writes each notification to a file."
command = ["send.sh"]
timeout_seconds = 10

[capabilities]
formats = ["plain"]
max_title_bytes = 40
max_body_bytes = 500

[capabilities.attachments]
max_count = 1
max_bytes = 64
images_only = true

[[settings]]
key = "webhook"
label = "Webhook"
type = "secret"

[[settings]]
key = "fail"
label = "Fail"
type = "choice"
choices = ["yes", "no"]
"#,
    )
    .unwrap();
    let script = dir.join("send.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\ninput=$(cat)\nprintf '%s\\n' \"$input\" >> '{}'\ncase \"$input\" in\n  *'\"fail\":\"yes\"'*) echo '{{\"ok\":false,\"error\":\"refused\",\"retryable\":false}}' ;;\n  *) echo '{{\"ok\":true,\"id\":\"rec-1\"}}' ;;\nesac\n",
            outbox.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn env() -> Env {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let outbox = root.path().join("outbox.jsonl");
    install_plugin(&home, &outbox);
    Env {
        _root: root,
        home,
        outbox,
    }
}

fn hs(env: &Env) -> Command {
    let mut cmd = Command::cargo_bin("hotsheet-cli").unwrap();
    cmd.arg("--test-fixture-credentials")
        .env("HOTSHEET_HOME", &env.home)
        .env_remove("HOTSHEET_ACTOR_ROLE")
        .env_remove("HOTSHEET_ACTOR_ID")
        .arg("notify");
    cmd
}

fn envelopes(env: &Env) -> Vec<Value> {
    std::fs::read_to_string(&env.outbox)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn headless_setup_send_test_and_removal_through_an_external_plugin() {
    let env = env();
    hs(&env)
        .args(["notifiers"])
        .assert()
        .success()
        .stdout(contains("speak — Speak aloud"))
        .stdout(contains("ntfy — ntfy push"))
        .stdout(contains("email — Email"))
        .stdout(contains("recorder — Recorder"));

    // Credentials are refused inline, stored by reference, and the add is idempotent.
    hs(&env)
        .args([
            "channel",
            "add",
            "rec",
            "--notifier",
            "recorder",
            "--set",
            "webhook=https://x",
        ])
        .assert()
        .failure()
        .stderr(contains("notify channel secret rec webhook"));
    for _ in 0..2 {
        hs(&env)
            .args([
                "channel",
                "add",
                "rec",
                "--notifier",
                "recorder",
                "--set",
                "fail=no",
            ])
            .assert()
            .success()
            .stdout(contains("notify channel secret rec webhook"));
    }
    hs(&env)
        .args(["channel", "secret", "rec", "webhook"])
        .write_stdin("https://hooks.example.test/abc\n")
        .assert()
        .success();
    let config = std::fs::read_to_string(env.home.join("notifications.json")).unwrap();
    assert!(!config.contains("hooks.example.test"), "{config}");
    // Re-adding keeps the stored credential reference.
    hs(&env)
        .args([
            "channel",
            "add",
            "rec",
            "--notifier",
            "recorder",
            "--set",
            "fail=no",
        ])
        .assert()
        .success();

    hs(&env)
        .args([
            "rule",
            "add",
            "stuck",
            "--event",
            "project.stopped",
            "--channel",
            "rec",
            "--cooldown",
            "600",
        ])
        .assert()
        .success();
    hs(&env)
        .args(["rule", "add", "bad", "--event", "test", "--channel", "rec"])
        .assert()
        .failure()
        .stderr(contains(
            "test notifications go only to the channel being tested",
        ));

    let image = env.home.join("shot.png");
    std::fs::write(&image, [0x89, b'P', b'N', b'G']).unwrap();
    let log = env.home.join("big.log");
    std::fs::write(&log, "x".repeat(10)).unwrap();
    let send = |env: &Env| {
        let mut cmd = hs(env);
        cmd.args([
            "send",
            "--event",
            "project.stopped",
            "--title",
            "Hot Sheet project Demo has stopped working on tickets",
            "--body",
            "No AI activity for **15 minutes**.",
            "--severity",
            "warning",
            "--dedupe-key",
            "stopped:demo",
            "--attach",
        ])
        .arg(&image)
        .arg("--attach")
        .arg(&log)
        .arg("--json");
        cmd
    };
    let out = send(&env).assert().success().get_output().stdout.clone();
    let records: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(records[0]["outcome"], "delivered");
    assert_eq!(records[0]["provider_id"], "rec-1");
    assert_eq!(records[0]["omitted_attachments"], 1);
    assert_eq!(records[0]["truncated"], true);

    let sent = envelopes(&env);
    assert_eq!(sent.len(), 1);
    let envelope = &sent[0];
    assert_eq!(envelope["protocol"], 1);
    assert_eq!(envelope["channel"]["id"], "rec");
    assert_eq!(
        envelope["settings"]["webhook"],
        "https://hooks.example.test/abc"
    );
    let message = &envelope["message"];
    assert_eq!(message["event"], "project.stopped");
    assert_eq!(message["severity"], "warning");
    assert_eq!(message["format"], "plain");
    assert!(
        message["title"]
            .as_str()
            .unwrap()
            .ends_with("… (truncated)")
    );
    assert_eq!(
        message["body"],
        "No AI activity for 15 minutes.\n\n1 attachment omitted: big.log"
    );
    assert_eq!(message["attachments"][0]["name"], "shot.png");

    // The same event inside the cooldown is suppressed; the plugin is not run again.
    let out = send(&env).assert().success().get_output().stdout.clone();
    let records: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(records[0]["outcome"], "suppressed");
    assert_eq!(envelopes(&env).len(), 1);

    // Nothing matches an event without a rule.
    hs(&env)
        .args(["send", "--title", "x", "--event", "digest"])
        .assert()
        .success()
        .stdout(contains("No enabled rule matched"));

    // A disabled channel still takes a test; a permanent plugin failure fails the command.
    hs(&env)
        .args(["channel", "disable", "rec"])
        .assert()
        .success();
    hs(&env)
        .args(["channel", "test", "rec"])
        .assert()
        .success()
        .stdout(contains("delivered to rec (recorder)"));
    assert_eq!(envelopes(&env).last().unwrap()["message"]["event"], "test");
    hs(&env)
        .args([
            "channel",
            "add",
            "rec",
            "--notifier",
            "recorder",
            "--set",
            "fail=yes",
        ])
        .assert()
        .success();
    hs(&env)
        .args(["channel", "test", "rec"])
        .assert()
        .failure()
        .stdout(contains(
            "failed for rec after 1 attempt(s): recorder: refused",
        ));

    let out = hs(&env)
        .args(["deliveries", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let deliveries: Value = serde_json::from_slice(&out).unwrap();
    let outcomes: Vec<_> = deliveries
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["outcome"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(outcomes, ["failed", "delivered", "suppressed", "delivered"]);
    assert!(
        !deliveries.to_string().contains("stopped working"),
        "the log holds no message text"
    );

    hs(&env)
        .args(["channel", "remove", "rec"])
        .assert()
        .success()
        .stdout(contains("removed rule stuck"))
        .stdout(contains("deleted credential notify-rec-webhook"));
    hs(&env)
        .args(["channel", "remove", "rec"])
        .assert()
        .success()
        .stdout(contains("No channel rec"));
    let out = hs(&env)
        .args(["config", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let config: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(config["channels"], serde_json::json!([]));
    assert_eq!(config["rules"], serde_json::json!([]));
}
