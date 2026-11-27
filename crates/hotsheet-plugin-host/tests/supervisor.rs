//! HS2-XY0VTN: the supervisor drives a real plugin process (`hs-plugin-diagnostics`) over
//! both transports through its whole lifecycle — trust, handshake failures, concurrency,
//! streams, deadlines, crashes, backoff, the circuit, idle exit and orderly stop. Waits
//! are on supervisor events, never fixed sleeps.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use hotsheet_plugin_host::{ALLOWED_ENV, CallStream, HostConfig, PluginHost, PluginStatus};
use hotsheet_plugin_protocol::diagnostics::method;
use hotsheet_plugin_protocol::{PluginError, Transport, code, v1};

const ID: &str = "diag";
const WAIT: Duration = Duration::from_secs(20);

struct Fixture {
    _root: tempfile::TempDir,
    home: PathBuf,
    plugins: PathBuf,
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().expect("temp dir");
    let home = root.path().join("home");
    let plugins = root.path().join("plugins");
    std::fs::create_dir_all(&home).expect("home");
    Fixture {
        home,
        plugins,
        _root: root,
    }
}

/// Writes a plugin directory whose executable is the diagnostics test plugin.
fn write_plugin(plugins: &Path, id: &str, args: &[&str], capabilities: &[&str]) -> PathBuf {
    let dir = plugins.join(id);
    std::fs::create_dir_all(dir.join("bin")).expect("plugin dir");
    // HS2-4TCTEW: the stable snapshot, never the uplifted target path.
    let exe = workspace_bin::path("hs-plugin-diagnostics");
    let exe = exe.as_path();
    let target = dir.join("bin").join(exe.file_name().expect("exe name"));
    #[cfg(unix)]
    std::os::unix::fs::symlink(exe, &target).expect("link plugin binary");
    #[cfg(not(unix))]
    std::fs::copy(exe, &target).expect("copy plugin binary");
    let quote = |items: &[&str]| {
        items
            .iter()
            .map(|item| format!("{item:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let manifest = format!(
        "id = \"{id}\"\ndisplay_name = \"Diagnostics\"\nversion = \"1.0.0\"\nprotocol = {{ min = 1, max = 1 }}\ncapabilities = [{}]\n\n[executable]\nprogram = \"bin/{}\"\nargs = [{}]\n",
        quote(capabilities),
        target.file_name().expect("name").to_string_lossy(),
        quote(args),
    );
    std::fs::write(dir.join("manifest.toml"), manifest).expect("manifest");
    dir
}

fn config(f: &Fixture) -> HostConfig {
    let mut config = HostConfig::new(f.home.clone(), "hotsheet-test", "0.0.0");
    config.handshake_timeout = Duration::from_secs(10);
    config.default_deadline = Duration::from_secs(10);
    config.shutdown_grace = Duration::from_secs(2);
    config.backoff_initial = Duration::from_millis(100);
    config.backoff_max = Duration::from_millis(400);
    config.circuit_crashes = 3;
    config.circuit_window = Duration::from_secs(120);
    config
}

/// A host with one approved diagnostics plugin.
fn host_with(f: &Fixture, config: HostConfig, args: &[&str]) -> Arc<PluginHost> {
    let dir = write_plugin(&f.plugins, ID, args, &["diagnostics"]);
    let host = PluginHost::new(config);
    host.register(&dir).expect("register");
    host.trust_store().approve(ID, &dir).expect("approve");
    host
}

fn echo(host: &Arc<PluginHost>, text: &str) -> Result<String, PluginError> {
    host.call::<_, v1::EchoResponse>(
        ID,
        method::ECHO,
        &v1::EchoRequest {
            text: text.into(),
            data: vec![7; 3],
        },
        None,
    )
    .map(|r| r.text)
}

fn exit(host: &Arc<PluginHost>) -> PluginError {
    host.call::<_, v1::Empty>(ID, method::EXIT, &v1::ExitRequest { status: 9 }, None)
        .expect_err("the plugin exits without answering")
}

fn running_pid(host: &PluginHost) -> u32 {
    match host.status(ID) {
        Some(PluginStatus::Running { pid, .. }) => pid,
        other => panic!("expected Running, got {other:?}"),
    }
}

fn wait_stopped(host: &PluginHost) {
    assert!(
        host.wait_for_status(ID, WAIT, |s| *s == PluginStatus::Stopped),
        "status stayed {:?}",
        host.status(ID)
    );
}

#[test]
fn hs2_xy0vtn_calls_work_on_both_transports_and_the_host_preference_wins() {
    for (offered, expected) in [
        (
            vec![Transport::Protobuf, Transport::Json],
            Transport::Protobuf,
        ),
        (vec![Transport::Json, Transport::Protobuf], Transport::Json),
    ] {
        let f = fixture();
        let mut config = config(&f);
        config.transports = offered;
        let host = host_with(&f, config, &[]);
        assert_eq!(host.status(ID), Some(PluginStatus::Stopped), "lazy start");
        assert_eq!(echo(&host, "hello").expect("echo"), "hello");
        match host.status(ID) {
            Some(PluginStatus::Running {
                transport,
                plugin_version,
                ..
            }) => {
                assert_eq!(transport, expected);
                assert!(!plugin_version.is_empty());
            }
            other => panic!("expected Running, got {other:?}"),
        }
    }
    // A plugin that only speaks JSON gets JSON even when the host prefers protobuf.
    let f = fixture();
    let host = host_with(&f, config(&f), &["--transports", "json"]);
    echo(&host, "x").expect("echo");
    assert!(matches!(
        host.status(ID),
        Some(PluginStatus::Running {
            transport: Transport::Json,
            ..
        })
    ));
}

#[test]
fn hs2_xy0vtn_streams_deliver_every_item_and_an_early_drop_cancels() {
    let f = fixture();
    let host = host_with(&f, config(&f), &[]);
    let stream: CallStream<v1::CountItem, v1::CountResponse> = host
        .stream(ID, method::COUNT, &v1::CountRequest { count: 2_000 }, None)
        .expect("stream");
    let mut seen = Vec::new();
    let mut stream = stream;
    for item in stream.by_ref() {
        seen.push(item.expect("item").seq);
    }
    assert_eq!(seen, (0..2_000).collect::<Vec<_>>());
    assert_eq!(stream.finish().expect("response").sent, 2_000);

    let mut endless: CallStream<v1::CountItem, v1::CountResponse> = host
        .stream(
            ID,
            method::COUNT,
            &v1::CountRequest { count: u64::MAX },
            None,
        )
        .expect("stream");
    for _ in 0..5 {
        endless.next().expect("item").expect("ok");
    }
    let pid = running_pid(&host);
    drop(endless);
    // The connection is still healthy after the cancel.
    assert_eq!(echo(&host, "after").expect("echo"), "after");
    assert_eq!(running_pid(&host), pid);
}

#[test]
fn hs2_xy0vtn_concurrent_calls_multiplex_and_deadlines_cancel() {
    let f = fixture();
    let host = host_with(&f, config(&f), &[]);
    echo(&host, "warm").expect("start");
    let slow = {
        let host = host.clone();
        std::thread::spawn(move || {
            host.call::<_, v1::Empty>(
                ID,
                method::SLEEP,
                &v1::SleepRequest { millis: 60_000 },
                Some(Duration::from_millis(300)),
            )
        })
    };
    let workers: Vec<_> = (0..8)
        .map(|n| {
            let host = host.clone();
            std::thread::spawn(move || echo(&host, &format!("w{n}")))
        })
        .collect();
    for (n, worker) in workers.into_iter().enumerate() {
        assert_eq!(
            worker.join().expect("thread").expect("echo"),
            format!("w{n}")
        );
    }
    let timed_out = slow.join().expect("thread").expect_err("deadline");
    assert_eq!(timed_out.code, code::DEADLINE_EXCEEDED);
    assert_eq!(echo(&host, "still up").expect("echo"), "still up");
}

#[test]
fn hs2_xy0vtn_plugin_errors_pass_through_unchanged() {
    let f = fixture();
    let host = host_with(&f, config(&f), &[]);
    let error = PluginError::rate_limited(Some(42));
    let got = host
        .call::<_, v1::Empty>(
            ID,
            method::FAIL,
            &v1::FailRequest {
                error: Some(error.clone()),
            },
            None,
        )
        .expect_err("fails");
    assert_eq!(got, error);
    let unsupported = host
        .call::<_, v1::Empty>(ID, "ticket_source.Get", &v1::Empty {}, None)
        .expect_err("unsupported");
    assert_eq!(unsupported.code, code::UNSUPPORTED);
    let unknown = host
        .call::<_, v1::Empty>("nope", method::ECHO, &v1::Empty {}, None)
        .expect_err("unknown");
    assert_eq!(unknown.code, code::NOT_FOUND);
}

#[test]
fn hs2_xy0vtn_the_plugin_sees_only_allowlisted_environment() {
    let f = fixture();
    let host = host_with(&f, config(&f), &[]);
    let env: v1::EnvironmentResponse = host
        .call(ID, method::ENVIRONMENT, &v1::Empty {}, None)
        .expect("environment");
    assert!(env.names.contains(&"HOTSHEET_PLUGIN_ID".to_string()));
    assert!(env.names.contains(&"HOTSHEET_PLUGIN_DATA_DIR".to_string()));
    for name in &env.names {
        assert!(
            ALLOWED_ENV.contains(&name.as_str())
                || name.starts_with("LC_")
                || name.starts_with("HOTSHEET_PLUGIN_"),
            "{name} leaked into the plugin"
        );
    }
    // The test process has HOTSHEET_HOME set by .cargo/config.toml; it must not leak.
    assert!(std::env::var_os("HOTSHEET_HOME").is_some());
    assert!(!env.names.contains(&"HOTSHEET_HOME".to_string()));
    assert!(
        f.home.join("plugin-data").join(ID).is_dir(),
        "data dir created"
    );
}

#[test]
fn hs2_xy0vtn_a_crash_fails_the_call_backs_off_and_restarts() {
    let f = fixture();
    let host = host_with(&f, config(&f), &["--stderr", "boom-marker"]);
    echo(&host, "up").expect("start");
    let first = running_pid(&host);
    let crashed = exit(&host);
    assert_eq!(crashed.code, code::UNAVAILABLE);
    assert!(
        crashed.message.contains("boom-marker"),
        "stderr tail: {}",
        crashed.message
    );
    assert!(
        host.wait_for_status(ID, WAIT, |s| !matches!(s, PluginStatus::Running { .. })),
        "the crash is noticed"
    );
    // During the backoff, calls fail fast with a retry hint.
    if let Some(PluginStatus::BackingOff { .. }) = host.status(ID) {
        let refused = echo(&host, "too soon").expect_err("backing off");
        assert_eq!(refused.code, code::UNAVAILABLE);
        assert!(refused.retry_after_seconds.is_some());
    }
    wait_stopped(&host);
    assert_eq!(echo(&host, "again").expect("restarted"), "again");
    assert_ne!(running_pid(&host), first);
}

#[test]
fn hs2_xy0vtn_repeated_crashes_open_the_circuit_until_reset() {
    let f = fixture();
    let host = host_with(&f, config(&f), &[]);
    for _ in 0..3 {
        wait_stopped(&host);
        echo(&host, "up").expect("start");
        exit(&host);
        assert!(host.wait_for_status(ID, WAIT, |s| !matches!(s, PluginStatus::Running { .. })));
    }
    assert!(matches!(
        host.status(ID),
        Some(PluginStatus::CircuitOpen { .. })
    ));
    let refused = echo(&host, "x").expect_err("disabled");
    assert!(refused.message.contains("disabled"), "{}", refused.message);
    assert!(host.reset(ID));
    assert_eq!(echo(&host, "back").expect("reset works"), "back");
}

#[test]
fn hs2_xy0vtn_an_idle_plugin_exits_and_restarts_on_demand() {
    let f = fixture();
    let mut config = config(&f);
    config.idle_timeout = Duration::from_millis(300);
    let host = host_with(&f, config, &[]);
    echo(&host, "up").expect("start");
    let first = running_pid(&host);
    // An idle exit is not a crash: straight to Stopped, no backoff.
    wait_stopped(&host);
    assert_eq!(echo(&host, "again").expect("restart"), "again");
    assert_ne!(running_pid(&host), first);
}

#[test]
fn hs2_xy0vtn_a_long_call_keeps_an_idle_plugin_alive() {
    let f = fixture();
    let mut config = config(&f);
    config.idle_timeout = Duration::from_millis(200);
    let host = host_with(&f, config, &[]);
    host.call::<_, v1::Empty>(ID, method::SLEEP, &v1::SleepRequest { millis: 600 }, None)
        .expect("a call longer than the idle timeout completes");
}

#[test]
fn hs2_xy0vtn_stop_is_orderly_and_not_a_crash() {
    let f = fixture();
    let host = host_with(&f, config(&f), &[]);
    echo(&host, "up").expect("start");
    host.stop(ID);
    assert_eq!(host.status(ID), Some(PluginStatus::Stopped));
    assert_eq!(
        echo(&host, "next").expect("restart without backoff"),
        "next"
    );
}

#[test]
fn hs2_xy0vtn_untrusted_or_changed_plugins_do_not_start() {
    let f = fixture();
    let dir = write_plugin(&f.plugins, ID, &[], &["diagnostics"]);
    let host = PluginHost::new(config(&f));
    host.register(&dir).expect("register");
    let refused = echo(&host, "x").expect_err("not approved");
    assert!(
        refused.message.contains("not trusted"),
        "{}",
        refused.message
    );
    // A trust refusal is a user decision, not a crash: no backoff.
    assert_eq!(host.status(ID), Some(PluginStatus::Stopped));

    host.trust_store().approve(ID, &dir).expect("approve");
    echo(&host, "ok").expect("approved");
    host.stop(ID);
    std::fs::write(dir.join("extra.txt"), "tampered").expect("change a file");
    let refused = echo(&host, "x").expect_err("changed");
    assert!(
        refused.message.contains("changed since it was trusted"),
        "{}",
        refused.message
    );
    assert!(host.check_trust(ID).is_err());

    // Plugins under a trusted root (first-party) need no approval.
    let f2 = fixture();
    let dir2 = write_plugin(&f2.plugins, ID, &[], &["diagnostics"]);
    let mut config2 = config(&f2);
    config2.trusted_roots = vec![f2.plugins.clone()];
    let host2 = PluginHost::new(config2);
    host2.register(&dir2).expect("register");
    assert_eq!(
        echo(&host2, "first-party").expect("trusted root"),
        "first-party"
    );
}

#[test]
fn hs2_xy0vtn_handshake_failures_are_reported_and_count_as_crashes() {
    let cases: [(&[&str], &[&str], &str); 3] = [
        (
            &["--exit-before-hello"],
            &["diagnostics"],
            "failed its handshake",
        ),
        (
            &["--capability", "notifier"],
            &["diagnostics"],
            "capabilities",
        ),
        (&["--hang-at-hello"], &["diagnostics"], "no answer within"),
    ];
    for (args, capabilities, expected) in cases {
        let f = fixture();
        let dir = write_plugin(&f.plugins, ID, args, capabilities);
        let mut config = config(&f);
        config.handshake_timeout = Duration::from_millis(500);
        let host = PluginHost::new(config);
        host.register(&dir).expect("register");
        host.trust_store().approve(ID, &dir).expect("approve");
        let error = echo(&host, "x").expect_err("handshake fails");
        assert_eq!(error.code, code::UNAVAILABLE);
        assert!(
            error.message.contains(expected),
            "{args:?}: {}",
            error.message
        );
        assert!(
            matches!(
                host.status(ID),
                Some(PluginStatus::BackingOff { .. }) | Some(PluginStatus::Stopped)
            ),
            "{:?}",
            host.status(ID)
        );
    }
}
