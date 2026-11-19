//! HS2-FMHH9Y headless E2E: `hotsheet-cli resource` against the local machine backend.
//! Every invocation is a separate process sharing one `HOTSHEET_HOME`, the way a holder,
//! a waiter and a task inside a VM would. Synthetic names only.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const PROMPT: Duration = Duration::from_secs(30);
const TOKEN_ENV: &str = "HOTSHEET_RESOURCE_CLAIM_TOKEN";

struct Env {
    _root: tempfile::TempDir,
    home: PathBuf,
}

fn env() -> Env {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    Env { _root: root, home }
}

fn command(home: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(workspace_bin::path("hotsheet-cli"));
    command
        .arg("resource")
        .args(args)
        .env("HOTSHEET_HOME", home)
        .env_remove(TOKEN_ENV)
        .env_remove("HOTSHEET_STORE")
        .current_dir(home.parent().unwrap())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

struct Run {
    ok: bool,
    json: Value,
    stderr: String,
}

fn finish(output: Output) -> Run {
    let stdout = String::from_utf8(output.stdout).unwrap();
    Run {
        ok: output.status.success(),
        json: serde_json::from_str(&stdout).unwrap_or(Value::Null),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

fn run(home: &Path, args: &[&str]) -> Run {
    finish(command(home, args).output().unwrap())
}

fn granted(home: &Path, args: &[&str]) -> Value {
    let run = run(home, args);
    assert!(run.ok, "{}", run.stderr);
    assert_eq!(run.json["acquired"], true, "{}", run.json);
    run.json
}

fn token(grant: &Value) -> &str {
    grant["renewal_token"].as_str().unwrap()
}

/// Start a waiting acquire and return once it has registered as a waiter, so the
/// release that follows exercises the cross-process wake-up rather than a lucky retry.
fn spawn_waiter(home: &Path, namespace: &str, args: &[&str]) -> Child {
    let child = command(home, args).spawn().unwrap();
    let waiters = home
        .join("resource-claims/namespaces")
        .join(namespace)
        .join("waiters");
    let started = Instant::now();
    while std::fs::read_dir(&waiters).map_or(0, Iterator::count) == 0 {
        assert!(started.elapsed() < PROMPT, "waiter never registered");
        std::thread::yield_now();
    }
    child
}

#[test]
fn hs2_fmhh9y_acquire_prints_the_grant_and_a_conflict_names_the_holder() {
    let env = env();
    let grant = granted(
        &env.home,
        &[
            "acquire",
            "vms/mac-1",
            "--duration",
            "30m",
            "--meta",
            "purpose=ui tests",
            "--meta",
            "worker=hmac-sha256:worker-1",
            "--meta",
            "label=enc:v1:Build agent",
        ],
    );
    assert!(grant["claim_id"].as_str().unwrap().len() == 26);
    assert_eq!(grant["fencing_token"], 1);
    assert!(token(&grant).starts_with("hsrc1."));
    assert!(grant["expires_at"].as_str().unwrap().ends_with('Z'));
    assert_eq!(
        grant["expires_at_ms"].as_u64().unwrap() - grant["acquired_at_ms"].as_u64().unwrap(),
        1_800_000
    );
    assert_eq!(grant["guarantee"], "exclusive_with_fencing");
    assert_eq!(grant["metadata"]["purpose"], "plain:ui tests");
    let worker = grant["metadata"]["worker"].as_str().unwrap();
    let label = grant["metadata"]["label"].as_str().unwrap();
    assert!(worker.starts_with("hmac-sha256:") && !worker.contains("worker-1"));
    assert!(label.starts_with("enc:v1:") && !label.contains("Build"));

    // Try-now conflict: a failed command whose stdout still names the holder.
    let refused = run(&env.home, &["acquire", "vms/mac-1", "--duration", "30m"]);
    assert!(!refused.ok);
    assert_eq!(refused.json["acquired"], false);
    assert_eq!(refused.json["code"], "held_by");
    assert_eq!(refused.json["holder"]["claim_id"], grant["claim_id"]);
    assert_eq!(refused.json["holder"]["metadata"], grant["metadata"]);
    assert_eq!(refused.json["holder"]["expires_at"], grant["expires_at"]);
    assert!(refused.stderr.contains("resource is held by claim"));
    for text in [refused.json.to_string(), refused.stderr] {
        assert!(
            !text.contains(token(&grant)),
            "a conflict never shows the token"
        );
    }

    let shown = run(&env.home, &["show", "vms/mac-1"]);
    assert!(shown.ok);
    assert_eq!(shown.json["held"], true);
    assert_eq!(shown.json["claim"]["claim_id"], grant["claim_id"]);
    assert_eq!(shown.json["capabilities"]["backend"], "local");
    let listed = run(&env.home, &["list", "vms"]);
    assert_eq!(listed.json["claims"].as_array().unwrap().len(), 1);
    let other = run(&env.home, &["list", "gpus"]);
    assert_eq!(other.json["claims"].as_array().unwrap().len(), 0);
    let free = run(&env.home, &["show", "vms/mac-2"]);
    assert_eq!(free.json["held"], false);
    for text in [shown.json.to_string(), listed.json.to_string()] {
        assert!(!text.contains(token(&grant)));
    }

    // The token is never written to disk; only its keyed digest is.
    let state = env.home.join("resource-claims/namespaces/vms/state.json");
    let stored = std::fs::read_to_string(state).unwrap();
    assert!(!stored.contains(token(&grant)));
    assert!(!stored.contains(token(&grant).rsplit('.').next().unwrap()));
}

#[test]
fn hs2_fmhh9y_token_alone_renews_and_releases_from_another_process() {
    let env = env();
    let grant = granted(&env.home, &["acquire", "vms/mac-1", "--duration", "1m"]);

    // A task that holds nothing but the token: by flag, then by environment.
    let renewed = run(
        &env.home,
        &["renew", "--token", token(&grant), "--duration", "10m"],
    );
    assert!(renewed.ok, "{}", renewed.stderr);
    assert_eq!(renewed.json["claim"]["claim_id"], grant["claim_id"]);
    assert_eq!(renewed.json["claim"]["fencing_token"], 1);
    assert!(renewed.json["claim"]["expires_at_ms"].as_u64() > grant["expires_at_ms"].as_u64());
    assert!(!renewed.json.to_string().contains(token(&grant)));

    let by_env = finish(
        command(&env.home, &["renew", "--duration", "20m"])
            .env(TOKEN_ENV, token(&grant))
            .output()
            .unwrap(),
    );
    assert!(by_env.ok, "{}", by_env.stderr);
    assert!(
        by_env.json["claim"]["expires_at_ms"].as_u64()
            > renewed.json["claim"]["expires_at_ms"].as_u64()
    );

    let missing = run(&env.home, &["renew", "--duration", "20m"]);
    assert!(!missing.ok);
    assert!(missing.stderr.contains(TOKEN_ENV), "{}", missing.stderr);

    let released = finish(
        command(&env.home, &["release"])
            .env(TOKEN_ENV, token(&grant))
            .output()
            .unwrap(),
    );
    assert!(released.ok, "{}", released.stderr);
    assert_eq!(released.json["released"], true);
    assert_eq!(run(&env.home, &["show", "vms/mac-1"]).json["held"], false);

    // Revoked: the same token does nothing after the release, even once re-acquired.
    for args in [
        vec!["release", "--token", token(&grant)],
        vec!["renew", "--token", token(&grant), "--duration", "1m"],
    ] {
        let revoked = run(&env.home, &args);
        assert!(!revoked.ok);
        assert_eq!(revoked.json["code"], "token_invalid", "{}", revoked.json);
        assert!(revoked.stderr.contains("token_invalid"));
    }
    let next = granted(&env.home, &["acquire", "vms/mac-1", "--duration", "1m"]);
    assert_eq!(next["fencing_token"], 2);
    let stale = run(&env.home, &["release", "--token", token(&grant)]);
    assert_eq!(stale.json["code"], "not_holder");
    assert_eq!(run(&env.home, &["show", "vms/mac-1"]).json["held"], true);
}

#[test]
fn hs2_fmhh9y_a_token_is_rejected_for_another_claim() {
    let env = env();
    let one = granted(&env.home, &["acquire", "vms/mac-1", "--duration", "5m"]);
    let two = granted(&env.home, &["acquire", "vms/mac-2", "--duration", "5m"]);

    // Claim one's token named against claim two.
    for args in [
        vec!["release", "vms/mac-2", "--token", token(&one)],
        vec![
            "renew",
            "vms/mac-2",
            "--token",
            token(&one),
            "--duration",
            "5m",
        ],
    ] {
        let refused = run(&env.home, &args);
        assert!(!refused.ok);
        assert_eq!(refused.json["code"], "token_invalid", "{}", refused.json);
    }
    // Claim one's secret re-addressed to claim two, and plain garbage.
    let parts_one: Vec<&str> = token(&one).split('.').collect();
    let parts_two: Vec<&str> = token(&two).split('.').collect();
    let forged = [parts_two[0], parts_two[1], parts_two[2], parts_one[3]].join(".");
    for bad in [forged.as_str(), "hsrc1.x.y.z", "nonsense"] {
        let refused = run(&env.home, &["release", "--token", bad]);
        assert!(!refused.ok);
        assert_eq!(refused.json["code"], "token_invalid", "{bad}");
    }
    // Both claims are still held, and each token still works for its own claim.
    let listed = run(&env.home, &["list", "vms"]);
    assert_eq!(listed.json["claims"].as_array().unwrap().len(), 2);
    assert!(run(&env.home, &["release", "vms/mac-2", "--token", token(&two)]).ok);
    assert!(run(&env.home, &["release", "--token", token(&one)]).ok);
    assert_eq!(
        run(&env.home, &["list", "vms"]).json["claims"],
        serde_json::json!([])
    );
}

#[test]
fn hs2_fmhh9y_wait_is_satisfied_by_a_release_in_another_process() {
    let env = env();
    let held = granted(&env.home, &["acquire", "vms/mac-1", "--duration", "10m"]);
    let started = Instant::now();
    let waiter = spawn_waiter(
        &env.home,
        "vms",
        &["acquire", "vms/mac-1", "--duration", "10m", "--wait", "5m"],
    );
    assert!(run(&env.home, &["release", "--token", token(&held)]).ok);

    let waited = finish(waiter.wait_with_output().unwrap());
    assert!(waited.ok, "{}", waited.stderr);
    assert!(started.elapsed() < PROMPT, "woken by the release");
    assert_eq!(waited.json["acquired"], true);
    assert_eq!(waited.json["fencing_token"], 2);
    assert_ne!(waited.json["claim_id"], held["claim_id"]);
}

#[test]
fn hs2_fmhh9y_wait_is_satisfied_by_expiry() {
    let env = env();
    let held = granted(&env.home, &["acquire", "vms/mac-1", "--duration", "1500ms"]);
    let waited = run(
        &env.home,
        &["acquire", "vms/mac-1", "--duration", "10m", "--wait", "5m"],
    );
    assert!(waited.ok, "{}", waited.stderr);
    assert_eq!(waited.json["fencing_token"], 2);
    assert!(waited.json["acquired_at_ms"].as_u64() >= held["expires_at_ms"].as_u64());
    // The lapsed holder is fenced out.
    let stale = run(
        &env.home,
        &["renew", "--token", token(&held), "--duration", "1m"],
    );
    assert_eq!(stale.json["code"], "not_holder");
}

#[test]
fn hs2_fmhh9y_wait_times_out_naming_the_holder() {
    let env = env();
    let held = granted(&env.home, &["acquire", "vms/mac-1", "--duration", "10m"]);
    let started = Instant::now();
    let refused = run(
        &env.home,
        &[
            "acquire",
            "vms/mac-1",
            "--duration",
            "10m",
            "--wait",
            "900ms",
        ],
    );
    assert!(started.elapsed() >= Duration::from_millis(900));
    assert!(!refused.ok);
    assert_eq!(refused.json["acquired"], false);
    assert_eq!(refused.json["code"], "wait_timeout");
    assert_eq!(refused.json["holder"]["claim_id"], held["claim_id"]);
    assert!(refused.stderr.contains("timed out waiting"));
    // The timed-out waiter left no registration behind.
    let waiters = env.home.join("resource-claims/namespaces/vms/waiters");
    assert_eq!(std::fs::read_dir(waiters).unwrap().count(), 0);

    // `show --wait` waits for free without claiming.
    let still = run(&env.home, &["show", "vms/mac-1", "--wait", "300ms"]);
    assert!(still.ok);
    assert_eq!(still.json["held"], true);
    let watcher = spawn_waiter(&env.home, "vms", &["show", "vms/mac-1", "--wait", "5m"]);
    assert!(run(&env.home, &["release", "--token", token(&held)]).ok);
    let freed = finish(watcher.wait_with_output().unwrap());
    assert!(freed.ok);
    assert_eq!(freed.json["held"], false);
    assert_eq!(
        granted(&env.home, &["acquire", "vms/mac-1", "--duration", "1m"])["fencing_token"],
        2,
        "watching never claimed"
    );
}

#[test]
fn hs2_fmhh9y_bad_input_is_refused_before_anything_is_written() {
    let env = env();
    for args in [
        vec!["acquire", "vms", "--duration", "1m"],
        vec!["acquire", "VMS/mac-1", "--duration", "1m"],
        vec!["acquire", "vms/mac-1", "--duration", "soon"],
        vec!["acquire", "vms/mac-1", "--duration", "0s"],
        vec!["acquire", "vms/mac-1", "--duration", "25h"],
        vec!["acquire", "vms/mac-1", "--duration", "1m", "--wait", "25h"],
        vec![
            "acquire",
            "vms/mac-1",
            "--duration",
            "1m",
            "--meta",
            "novalue",
        ],
        vec![
            "acquire",
            "vms/mac-1",
            "--duration",
            "1m",
            "--meta",
            "a=1",
            "--meta",
            "a=2",
        ],
        vec![
            "acquire",
            "vms/mac-1",
            "--duration",
            "1m",
            "--meta",
            "Bad Key=1",
        ],
        vec!["list", "vms/mac-1"],
        vec!["show", "vms"],
    ] {
        let refused = run(&env.home, &args);
        assert!(!refused.ok, "{args:?}");
        assert!(!refused.stderr.is_empty(), "{args:?}");
    }
    assert!(!env.home.join("resource-claims/namespaces").exists());
}

// ---- HS2-QGY1NN: a stopped wait removes its registration -----------------------------

#[cfg(unix)]
fn waiters_left(home: &Path, namespace: &str) -> usize {
    std::fs::read_dir(
        home.join("resource-claims/namespaces")
            .join(namespace)
            .join("waiters"),
    )
    .map_or(0, |entries| {
        entries
            .filter_map(Result::ok)
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .count()
    })
}

#[cfg(unix)]
fn stopped_by(child: &mut Child) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.signal();
        }
        assert!(started.elapsed() < PROMPT, "the stopped wait never exited");
        std::thread::yield_now();
    }
}

#[cfg(unix)]
#[test]
fn hs2_qgy1nn_a_wait_ended_by_a_stop_signal_removes_its_registration_and_dies_by_the_signal() {
    let e = env();
    let held = granted(&e.home, &["acquire", "vms/one", "--duration", "300s"]);
    for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        let mut waiter = spawn_waiter(
            &e.home,
            "vms",
            &["acquire", "vms/one", "--duration", "60s", "--wait", "600s"],
        );
        // `spawn_waiter` returns on the staged file; wait for the published registration.
        let started = Instant::now();
        while waiters_left(&e.home, "vms") < 1 {
            assert!(started.elapsed() < PROMPT, "the wait never registered");
            std::thread::yield_now();
        }
        // SAFETY: signalling a child process this test spawned and still owns.
        assert_eq!(
            unsafe { libc::kill(i32::try_from(waiter.id()).unwrap(), signal) },
            0
        );
        assert_eq!(
            stopped_by(&mut waiter),
            Some(signal),
            "the process still dies by the signal it was sent"
        );
        assert_eq!(
            waiters_left(&e.home, "vms"),
            0,
            "signal {signal}: the registration was removed, not left to its deadline"
        );
    }
    // The holder was never disturbed, and a stopped waiter did not use up the namespace.
    let shown = run(&e.home, &["show", "vms/one"]);
    assert!(shown.ok, "{}", shown.stderr);
    assert_eq!(shown.json["claim"]["claim_id"], held["claim_id"]);
}

#[cfg(unix)]
#[test]
fn hs2_qgy1nn_many_stopped_waits_never_fill_the_namespace() {
    let e = env();
    granted(&e.home, &["acquire", "vms/one", "--duration", "300s"]);
    let mut waiters: Vec<Child> = (0..6)
        .map(|_| {
            command(
                &e.home,
                &["acquire", "vms/one", "--duration", "60s", "--wait", "600s"],
            )
            .spawn()
            .unwrap()
        })
        .collect();
    let started = Instant::now();
    while waiters_left(&e.home, "vms") < 6 {
        assert!(started.elapsed() < PROMPT, "not every waiter registered");
        std::thread::yield_now();
    }
    for waiter in &waiters {
        // SAFETY: signalling children this test spawned and still owns.
        unsafe { libc::kill(i32::try_from(waiter.id()).unwrap(), libc::SIGTERM) };
    }
    for waiter in &mut waiters {
        assert_eq!(stopped_by(waiter), Some(libc::SIGTERM));
    }
    assert_eq!(waiters_left(&e.home, "vms"), 0);
    // A wait that ends normally is unchanged: it still times out naming the holder.
    let timed_out = run(
        &e.home,
        &["acquire", "vms/one", "--duration", "60s", "--wait", "300ms"],
    );
    assert!(!timed_out.ok);
    assert_eq!(timed_out.json["code"], "wait_timeout", "{}", timed_out.json);
    assert_eq!(waiters_left(&e.home, "vms"), 0);
}
