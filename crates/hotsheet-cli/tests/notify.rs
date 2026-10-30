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
        .stdout(contains("speak — Speak Aloud"))
        .stdout(contains("ntfy — ntfy Push"))
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

    // HS2-208RB1: an edited plugin is refused without running until the channel trusts it.
    let script = env.home.join("notifiers/recorder/send.sh");
    let mut text = std::fs::read_to_string(&script).unwrap();
    text.push_str("# edited after approval\n");
    std::fs::write(&script, text).unwrap();
    let before = envelopes(&env).len();
    hs(&env)
        .args(["channel", "test", "rec"])
        .assert()
        .failure()
        .stdout(contains("changed since channel 'rec' approved it"));
    assert_eq!(
        envelopes(&env).len(),
        before,
        "the changed plugin did not run"
    );
    hs(&env)
        .args(["config"])
        .assert()
        .success()
        .stdout(contains("[plugin changed: review it"));
    hs(&env)
        .args(["channel", "trust", "rec"])
        .assert()
        .success()
        .stdout(contains(
            "Approved notifier plugin recorder for channel rec",
        ));
    hs(&env).args(["channel", "test", "rec"]).assert().success();
    assert_eq!(envelopes(&env).len(), before + 1);
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
    assert_eq!(
        outcomes,
        [
            "failed",
            "delivered",
            "failed",
            "delivered",
            "suppressed",
            "delivered"
        ]
    );
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

/// One HTTP request the loopback listener received.
#[derive(Debug)]
struct Received {
    line: String,
    headers: Vec<(String, String)>,
    body: String,
}

/// A loopback HTTP server that answers every request `201 {"sid":"SMx"}` and reports what it
/// got, so the CLI's live HTTP transport runs without the internet (HS2-97ZHMM).
fn loopback() -> (String, std::sync::mpsc::Receiver<Received>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut headers = Vec::new();
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                let header = header.trim_end().to_string();
                if header.is_empty() {
                    break;
                }
                let (name, value) = header.split_once(':').unwrap();
                headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
            }
            let length: usize = headers
                .iter()
                .find(|(n, _)| n == "content-length")
                .map_or(0, |(_, v)| v.parse().unwrap());
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let reply = r#"{"sid":"SMx"}"#;
            write!(
                stream,
                "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            )
            .unwrap();
            let _ = tx.send(Received {
                line: line.trim_end().to_string(),
                headers,
                body: String::from_utf8(body).unwrap(),
            });
        }
    });
    (base, rx)
}

#[test]
fn hs2_97zhmm_webhook_slack_teams_and_sms_deliver_over_live_http() {
    let env = env();
    let (base, received) = loopback();
    let add = |args: &[&str]| {
        hs(&env)
            .args(["channel", "add"])
            .args(args)
            .assert()
            .success();
    };
    let secret = |channel: &str, field: &str, value: &str| {
        hs(&env)
            .args(["channel", "secret", channel, field])
            .write_stdin(format!("{value}\n"))
            .assert()
            .success();
    };
    add(&["hook", "--notifier", "webhook"]);
    secret("hook", "url", &format!("{base}/hook"));
    secret("hook", "signing_secret", "s3");
    add(&["slack", "--notifier", "slack"]);
    secret("slack", "webhook_url", &format!("{base}/slack"));
    add(&["teams", "--notifier", "teams"]);
    secret("teams", "webhook_url", &format!("{base}/teams"));
    let api = format!("api_base={base}");
    add(&[
        "sms",
        "--notifier",
        "twilio",
        "--set",
        "account_sid=AC1",
        "--set",
        "from=+15550000000",
        "--set",
        "to=+15551110000,+15552220000",
        "--set",
        &api,
    ]);
    secret("sms", "auth_token", "tok");
    hs(&env)
        .args([
            "rule",
            "add",
            "digest",
            "--event",
            "digest",
            "--channel",
            "hook",
            "--channel",
            "slack",
            "--channel",
            "teams",
            "--channel",
            "sms",
        ])
        .assert()
        .success();

    let out = hs(&env)
        .args([
            "send",
            "--event",
            "digest",
            "--title",
            "Daily digest",
            "--body",
            "**3** tickets done",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let records: Value = serde_json::from_slice(&out).unwrap();
    let delivered: Vec<_> = records
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["channel"].as_str().unwrap().to_string(),
                r["outcome"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        delivered,
        [
            ("hook", "delivered"),
            ("slack", "delivered"),
            ("teams", "delivered"),
            ("sms", "delivered")
        ]
        .map(|(c, o)| (c.to_string(), o.to_string()))
    );

    let requests: Vec<Received> = (0..5)
        .map(|_| {
            received
                .recv_timeout(std::time::Duration::from_secs(20))
                .unwrap()
        })
        .collect();
    let by_path = |path: &str| {
        requests
            .iter()
            .filter(|r| r.line.contains(path))
            .collect::<Vec<_>>()
    };
    let header = |r: &Received, name: &str| {
        r.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    };

    let hook = by_path(" /hook ")[0];
    let body: Value = serde_json::from_str(&hook.body).unwrap();
    assert_eq!(
        (body["event"].as_str(), body["title"].as_str()),
        (Some("digest"), Some("Daily digest"))
    );
    assert!(
        header(hook, "x-hotsheet-signature")
            .unwrap()
            .starts_with("sha256=")
    );

    let slack: Value = serde_json::from_str(&by_path(" /slack ")[0].body).unwrap();
    assert_eq!(slack["blocks"][1]["text"]["text"], "*3* tickets done");

    let teams: Value = serde_json::from_str(&by_path(" /teams ")[0].body).unwrap();
    assert_eq!(
        teams["attachments"][0]["content"]["body"][0]["text"],
        "Daily digest"
    );

    let sms = by_path("/2010-04-01/Accounts/AC1/Messages.json");
    assert_eq!(sms.len(), 2, "one message per recipient");
    assert_eq!(
        header(sms[0], "authorization").as_deref(),
        Some("Basic QUMxOnRvaw==")
    );
    assert!(sms.iter().any(|r| r.body.starts_with("To=%2B15552220000&")));
    assert!(
        sms[0]
            .body
            .contains("Body=Daily%20digest%0A%0A3%20tickets%20done"),
        "{}",
        sms[0].body
    );
}

#[test]
fn hs2_4jg95g_quiet_hours_rate_limits_and_text_rules_from_the_cli() {
    let env = env();
    let (base, received) = loopback();
    let records = |args: &[&str]| -> Vec<(String, Option<String>)> {
        let out = hs(&env)
            .args(args)
            .arg("--json")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let value: Value = serde_json::from_slice(&out).unwrap();
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["outcome"].as_str().unwrap().to_string(),
                    r["error"].as_str().map(str::to_owned),
                )
            })
            .collect()
    };
    // A quiet window around the current UTC time, so the test never depends on the hour.
    let now = time::OffsetDateTime::now_utc();
    let hhmm = |t: time::OffsetDateTime| format!("{:02}:{:02}", t.hour(), t.minute());
    let window = format!(
        "{}-{}",
        hhmm(now - time::Duration::hours(1)),
        hhmm(now + time::Duration::hours(1))
    );
    hs(&env)
        .args([
            "channel",
            "add",
            "quiet",
            "--notifier",
            "webhook",
            "--quiet",
            &window,
            "--quiet-offset",
            "+00:00",
        ])
        .assert()
        .success();
    hs(&env)
        .args(["channel", "secret", "quiet", "url"])
        .write_stdin(format!("{base}/quiet\n"))
        .assert()
        .success();
    hs(&env)
        .args([
            "channel",
            "add",
            "limited",
            "--notifier",
            "webhook",
            "--max-per-hour",
            "1",
        ])
        .assert()
        .success();
    hs(&env)
        .args(["channel", "secret", "limited", "url"])
        .write_stdin(format!("{base}/limited\n"))
        .assert()
        .success();
    hs(&env)
        .args([
            "rule",
            "add",
            "all",
            "--event",
            "notification",
            "--channel",
            "quiet",
            "--channel",
            "limited",
        ])
        .assert()
        .success();
    hs(&env)
        .args([
            "rule",
            "add",
            "release",
            "--event",
            "digest",
            "--channel",
            "limited",
            "--text",
            "release",
        ])
        .assert()
        .success();

    let first = records(&["send", "--title", "one"]);
    assert_eq!(first[0].0, "suppressed");
    assert!(
        first[0].1.as_deref().unwrap().starts_with("quiet hours"),
        "{first:?}"
    );
    assert_eq!(first[1], ("delivered".to_string(), None));
    let second = records(&["send", "--title", "two"]);
    assert_eq!(
        second[1],
        (
            "suppressed".to_string(),
            Some("rate limit of 1 per hour reached".into())
        )
    );
    // An explicit test ignores quiet hours.
    hs(&env)
        .args(["channel", "test", "quiet"])
        .assert()
        .success();
    // A text rule needs a ticket to match; a bare send has none.
    hs(&env)
        .args(["send", "--event", "digest", "--title", "release notes"])
        .assert()
        .success()
        .stdout(contains("No enabled rule matched"));
    let paths: Vec<String> = (0..2)
        .map(|_| {
            received
                .recv_timeout(std::time::Duration::from_secs(20))
                .unwrap()
                .line
        })
        .collect();
    assert!(
        paths.iter().any(|l| l.contains(" /limited "))
            && paths.iter().any(|l| l.contains(" /quiet ")),
        "{paths:?}"
    );
    assert!(
        received
            .recv_timeout(std::time::Duration::from_millis(500))
            .is_err(),
        "nothing else was sent"
    );
    hs(&env)
        .args([
            "channel",
            "add",
            "x",
            "--notifier",
            "webhook",
            "--quiet",
            "late",
        ])
        .assert()
        .failure()
        .stderr(contains("--quiet expects HH:MM-HH:MM"));
}

#[test]
fn hs2_04j8ws_link_base_is_set_shown_validated_and_cleared() {
    let env = env();
    hs(&env)
        .args(["link-base"])
        .assert()
        .success()
        .stdout(contains("notifications carry no links"));
    std::fs::create_dir_all(&env.home).unwrap();
    std::fs::write(
        env.home.join("client.json"),
        r#"{"url":"http://127.0.0.1:4174"}"#,
    )
    .unwrap();
    hs(&env)
        .args(["link-base"])
        .assert()
        .success()
        .stdout(contains("local client at http://127.0.0.1:4174"));
    hs(&env)
        .args(["link-base", "https://hotsheet.lan:8443/"])
        .assert()
        .success()
        .stdout(contains("Links open https://hotsheet.lan:8443"));
    hs(&env)
        .args(["link-base", "hotsheet.lan"])
        .assert()
        .failure()
        .stderr(contains("link_base must be an http(s) origin"));
    hs(&env)
        .args(["link-base", "--clear"])
        .assert()
        .success()
        .stdout(contains("local client at"));
}

#[test]
fn hs2_p57xhv_project_timing_is_shown_set_and_validated() {
    let env = env();
    hs(&env)
        .args(["project-timing"])
        .assert()
        .success()
        .stdout(contains("grace: 120 s; idle while claimed: 30 min"));
    hs(&env)
        .args([
            "project-timing",
            "--grace-seconds",
            "300",
            "--idle-minutes",
            "0",
        ])
        .assert()
        .success()
        .stdout(contains("grace: 300 s; idle while claimed: off"));
    hs(&env)
        .args(["project-timing", "--idle-minutes", "45"])
        .assert()
        .success()
        .stdout(contains("grace: 300 s; idle while claimed: 45 min"));
    hs(&env)
        .args(["project-timing", "--grace-seconds", "0"])
        .assert()
        .failure()
        .stderr(contains("grace_seconds must be 1-86400"));
}

/// HS2-8VAXXA: with no live server for the store, CLI ticket writes deliver their own
/// transitions; a live server's registration hands routing back to the server.
#[test]
fn hs2_8vaxxa_cli_writes_notify_without_a_server_and_defer_to_a_live_one() {
    let env = env();
    let (base, received) = loopback();
    let store = env.home.parent().unwrap().join("demo.hs2");
    hotsheet_ticketing::FsStore::init(&store, &hotsheet_ticketing::StoreMetadata::new("HS"))
        .unwrap();
    let tickets = || {
        let mut cmd = Command::cargo_bin("hotsheet-cli").unwrap();
        cmd.arg("--test-fixture-credentials")
            .env("HOTSHEET_HOME", &env.home)
            .env_remove("HOTSHEET_ACTOR_ROLE")
            .env_remove("HOTSHEET_ACTOR_ID")
            .env_remove("HOTSHEET_STORE")
            .arg("-C")
            .arg(&store);
        cmd
    };
    hs(&env)
        .args(["channel", "add", "hook", "--notifier", "webhook"])
        .assert()
        .success();
    hs(&env)
        .args(["channel", "secret", "hook", "url"])
        .write_stdin(format!("{base}/hook\n"))
        .assert()
        .success();
    hs(&env)
        .args([
            "rule",
            "add",
            "tickets",
            "--event",
            "ticket.status_changed",
            "--event",
            "ticket.blocked",
            "--channel",
            "hook",
        ])
        .assert()
        .success();
    hs(&env)
        .args(["link-base", "http://hotsheet.lan:4174"])
        .assert()
        .success();

    let out = tickets()
        .args(["new", "Ship it"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let slug = String::from_utf8(out)
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_string();
    tickets()
        .args(["edit", &slug, "--status", "started"])
        .assert()
        .success();
    let request = received
        .recv_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["event"], "ticket.status_changed");
    assert_eq!(body["title"], format!("{slug} is now Started"));
    assert_eq!(body["project_name"], "demo");
    assert!(
        body["url"]
            .as_str()
            .unwrap()
            .starts_with("http://hotsheet.lan:4174/?project=")
    );
    assert!(
        body["url"]
            .as_str()
            .unwrap()
            .ends_with(&format!("&ticket={slug}"))
    );

    // A read-only command sends nothing; a blocked edit sends exactly one message.
    tickets().args(["ls"]).assert().success();
    tickets()
        .args(["edit", &slug, "--blocked-reason", "waiting on review"])
        .assert()
        .success();
    let request = received
        .recv_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["event"], "ticket.blocked");
    assert!(
        received
            .recv_timeout(std::time::Duration::from_millis(500))
            .is_err(),
        "nothing else"
    );

    // A live server hosting the store owns routing: the CLI stays quiet.
    let instances = env.home.join("instances");
    std::fs::create_dir_all(&instances).unwrap();
    std::fs::write(
        instances.join("live.json"),
        serde_json::json!({"pid": std::process::id(), "url": "http://127.0.0.1:1", "store_path": store, "started_at": "now"}).to_string(),
    )
    .unwrap();
    tickets()
        .args(["edit", &slug, "--status", "completed"])
        .assert()
        .success();
    assert!(
        received
            .recv_timeout(std::time::Duration::from_millis(800))
            .is_err(),
        "the server would send this"
    );
}

/// HS2-G8NXNC: a rule naming a checkout's saved view forwards only transitions of tickets in
/// that view, evaluated by the serverless CLI write path; `--view` resolves and checks the view.
#[test]
fn hs2_g8nxnc_saved_view_rules_forward_only_tickets_in_the_view() {
    let env = env();
    let (base, received) = loopback();
    let root = env.home.parent().unwrap();
    let store = root.join("demo.hs2");
    let project = root.join("demo");
    std::fs::create_dir_all(&project).unwrap();
    hotsheet_ticketing::FsStore::init(&store, &hotsheet_ticketing::StoreMetadata::new("HS"))
        .unwrap();
    let checkout =
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(env.home.join("checkouts.json"))
            .register(&project, None, None, vec![store.clone()])
            .unwrap();
    hotsheet_ticketing::saved_views::replace(
        &checkout.settings(),
        &[hotsheet_ticketing::saved_views::SavedView {
            id: "started-docs".into(),
            name: "Started docs".into(),
            query: "tag:docs AND is:started".into(),
        }],
    )
    .unwrap();
    let tickets = || {
        let mut cmd = Command::cargo_bin("hotsheet-cli").unwrap();
        cmd.arg("--test-fixture-credentials")
            .env("HOTSHEET_HOME", &env.home)
            .env_remove("HOTSHEET_ACTOR_ROLE")
            .env_remove("HOTSHEET_ACTOR_ID")
            .env_remove("HOTSHEET_STORE")
            .arg("-C")
            .arg(&store);
        cmd
    };
    hs(&env)
        .args(["channel", "add", "hook", "--notifier", "webhook"])
        .assert()
        .success();
    hs(&env)
        .args(["channel", "secret", "hook", "url"])
        .write_stdin(format!("{base}/hook\n"))
        .assert()
        .success();
    let rule = |view: &str| {
        let mut cmd = hs(&env);
        cmd.args([
            "rule",
            "add",
            "docs",
            "--event",
            "ticket.status_changed",
            "--channel",
            "hook",
            "--view",
            view,
        ]);
        cmd
    };
    rule("no-slash")
        .assert()
        .failure()
        .stderr(contains("CHECKOUT/VIEW"));
    rule(&format!("{}/missing", checkout.id))
        .assert()
        .failure()
        .stderr(contains("saved views: started-docs"));
    // The alias resolves to the checkout id; the view id is matched ignoring case.
    rule("demo/STARTED-DOCS").assert().success();
    let config = std::fs::read_to_string(env.home.join("notifications.json")).unwrap();
    let config: Value = serde_json::from_str(&config).unwrap();
    assert_eq!(
        config["rules"][0]["filter"]["views"],
        serde_json::json!([{"checkout": checkout.id, "view": "started-docs"}])
    );

    let slug = |out: Vec<u8>| {
        String::from_utf8(out)
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_string()
    };
    let docs = slug(
        tickets()
            .args(["new", "Write the guide", "--tag", "docs"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    let other = slug(
        tickets()
            .args(["new", "Fix a crash"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    tickets()
        .args(["edit", &other, "--status", "started"])
        .assert()
        .success();
    tickets()
        .args(["edit", &docs, "--status", "started"])
        .assert()
        .success();
    let request = received
        .recv_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["title"], format!("{docs} is now Started"));
    // Completing leaves the view (it is no longer started), so nothing more is sent.
    tickets()
        .args(["edit", &docs, "--status", "completed"])
        .assert()
        .success();
    assert!(
        received
            .recv_timeout(std::time::Duration::from_millis(800))
            .is_err(),
        "only the docs ticket's start is in the view"
    );
}
