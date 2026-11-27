//! HS2-322ZSZ: the external claim-backend plugin protocol against real subprocesses.
//!
//! The fake plugin is this test binary, run directly by the manifest's command as its
//! ignored `fake_plugin_main` test, so it speaks the protocol on stdin and stdout with no
//! shell in between (HS2-XQHTXC: the same command on Unix and Windows; run any other way it
//! does nothing). Its state is a JSON file under a per-test directory, serialized by a file
//! lock, so the plugin is a real backend: atomic acquire, a fencing counter that survives
//! release and expiry, and its own clock. The children it starts for the process-tree
//! cases are this binary again (`fake_sleeper`). Nothing here touches a credential store,
//! and every name is synthetic.

use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use hotsheet_resource_claims::conformance::{self, Fixture};
use hotsheet_resource_claims::external::{
    self, ExternalBackend, LiveRunner, PluginManifest, SharedSecrets, approve, load_all,
};
use hotsheet_resource_claims::{
    AcquireRequest, ClaimBackend, ClaimError, Namespace, OfflinePolicy, ResourceRef, WaitCancel,
    WaitMechanism, WaitOutcome, resolve,
};
use hotsheet_ticketing::FileLock;
use hotsheet_ticketing::claim_backends::{BackendConfiguration, BackendKind};
use hotsheet_ticketing::secrets::{SecretError, SecretStore};

/// HS2-XQHTXC: set on a `fake_sleeper` child only, so `--run-ignored all` never sleeps.
const SLEEPER: &str = "HS_FAKE_SLEEPER";

/// A synthetic credential store: HS2-XTQHPK denies the native one in test processes.
#[derive(Default)]
struct MemorySecrets(std::sync::Mutex<std::collections::BTreeMap<String, String>>);

impl SecretStore for MemorySecrets {
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        self.0.lock().unwrap().insert(account.into(), secret.into());
        Ok(())
    }
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }
    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        Ok(self.0.lock().unwrap().remove(account).is_some())
    }
}

fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

// ---- the fake plugin ----------------------------------------------------------------

/// The plugin entry point, which the manifest's command runs. Run any other way (an
/// interactive `--ignored`, or `--run-ignored all` with no stdin) it does nothing.
#[test]
#[ignore = "HS2-XQHTXC: launched only by the fake plugin's manifest command"]
fn fake_plugin_main() {
    if std::io::stdin().is_terminal() {
        return;
    }
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    let Ok(envelope) = serde_json::from_str::<Value>(&input) else {
        return;
    };
    if envelope.get("protocol").is_none() {
        return;
    }
    let state_dir = PathBuf::from(envelope["settings"]["state_dir"].as_str().unwrap());
    // Record every run: its raw stdin, argv and environment, for what the plugin saw.
    let seen = json!({
        "stdin": input,
        "args": std::env::args().collect::<Vec<_>>(),
        "env": std::env::vars().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>(),
        "pid": std::process::id(),
    });
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state_dir.join("runs.log"))
        .unwrap();
    std::io::Write::write_all(&mut log, format!("{seen}\n").as_bytes()).unwrap();

    let mode = std::fs::read_to_string(state_dir.join("mode")).unwrap_or_default();
    match mode.trim() {
        "crash" => std::process::exit(3),
        "garbage" => {
            println!("\nthis is not a result");
            return;
        }
        "hang" => std::thread::sleep(Duration::from_secs(120)),
        // HS2-CYDK9Z: start a child of its own that shares the output pipes, record its
        // pid, then wait for it; a stop must end the child too.
        "forks" => {
            let mut child = sleeper()
                .stdin(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let pid_file = state_dir.join("grandchild.pid");
            std::fs::write(pid_file.with_extension("tmp"), child.id().to_string()).unwrap();
            std::fs::rename(pid_file.with_extension("tmp"), pid_file).unwrap();
            // Hangs as long as its child sleeps.
            let _ = child.wait();
        }
        // HS2-BR14FM: start a child that outlives this run, record its pid, then answer
        // and exit normally. `leaves_child` shares the output pipes with it;
        // `leaves_quiet_child` gives it none.
        "leaves_child" | "leaves_quiet_child" => {
            let mut command = sleeper();
            command.stdin(std::process::Stdio::null());
            if mode.trim() == "leaves_quiet_child" {
                command
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
            }
            #[expect(clippy::zombie_processes, reason = "HS2-BR14FM: the runner ends it")]
            let child = command.spawn().unwrap();
            let pid_file = state_dir.join("grandchild.pid");
            std::fs::write(pid_file.with_extension("tmp"), child.id().to_string()).unwrap();
            std::fs::rename(pid_file.with_extension("tmp"), pid_file).unwrap();
        }
        "ok_but_failed_exit" => {
            println!("\n{}", json!({ "ok": true, "result": {} }));
            std::process::exit(2);
        }
        _ => {}
    }
    let reply = handle(&state_dir, &envelope, mode.trim());
    println!("\n{reply}");
}

/// HS2-XQHTXC: a child of the plugin that sleeps two minutes: this binary's ignored
/// `fake_sleeper`, so it runs on every platform. It inherits the plugin's output handles
/// unless the caller redirects them.
fn sleeper() -> std::process::Command {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", "fake_sleeper", "--nocapture"])
        .env(SLEEPER, "1");
    command
}

/// The plugin's child for the process-tree cases. Run any other way it does nothing.
#[test]
#[ignore = "HS2-XQHTXC: launched only by the fake plugin as its own child"]
fn fake_sleeper() {
    if std::env::var_os(SLEEPER).is_some() {
        std::thread::sleep(Duration::from_secs(120));
    }
}

fn err(code: &str, message: &str) -> Value {
    json!({ "ok": false, "error": { "code": code, "message": message } })
}

fn wire(resource: &Value) -> Value {
    json!({
        "claim_id": resource["claim_id"],
        "fencing_token": resource["fencing"],
        "acquired_at_ms": resource["acquired"],
        "expires_at_ms": resource["expires"],
        "metadata": resource["metadata"],
    })
}

fn live(slot: &Value, now: u64) -> bool {
    slot["claim"]["expires"].as_u64().is_some_and(|e| e > now)
}

fn handle(state_dir: &Path, envelope: &Value, mode: &str) -> Value {
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(state_dir.join("lock"))
        .unwrap();
    let _lock = FileLock::acquire(lock_file).unwrap();
    let path = state_dir.join("state.json");
    let mut state: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}));
    let request = &envelope["request"];
    let target = request["target"].as_str().unwrap_or_default().to_owned();
    let now = now_ms();
    let reply = match envelope["op"].as_str().unwrap() {
        "acquire" => {
            let slot = state[&target].clone();
            if live(&slot, now) {
                json!({ "ok": false, "error": {
                    "code": "held_by", "message": "held",
                    "holder": wire(&slot["claim"]),
                } })
            } else {
                let fence = slot["fence"].as_u64().unwrap_or(0) + 1;
                let mut claim = json!({
                    "claim_id": request["claim_id"],
                    "fencing": fence,
                    "acquired": now,
                    "expires": now + request["duration_ms"].as_u64().unwrap(),
                    "metadata": request["metadata"],
                    "digest": request["token_digest"],
                });
                match mode {
                    "fencing_zero" => claim["fencing"] = json!(0),
                    "wrong_claim" => claim["claim_id"] = json!("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
                    _ => {}
                }
                state[&target] = json!({ "fence": fence, "claim": claim });
                json!({ "ok": true, "result": { "claim": wire(&state[&target]["claim"]) } })
            }
        }
        "renew" | "release" => {
            let slot = state[&target].clone();
            let claim = &slot["claim"];
            if claim.is_null() {
                err("token_invalid", "no claim")
            } else if claim["claim_id"] != request["claim_id"] {
                err("not_holder", "another claim holds it")
            } else if claim["digest"] != request["token_digest"] {
                err("token_invalid", "digest")
            } else if !live(&slot, now) {
                if envelope["op"] == "release" {
                    state[&target]["claim"] = Value::Null;
                }
                err("expired", "lapsed")
            } else if envelope["op"] == "renew" {
                let expires = now + request["duration_ms"].as_u64().unwrap();
                state[&target]["claim"]["expires"] = json!(expires);
                json!({ "ok": true, "result": { "claim": wire(&state[&target]["claim"]) } })
            } else {
                let ended = wire(claim);
                state[&target]["claim"] = Value::Null;
                json!({ "ok": true, "result": { "claim": ended } })
            }
        }
        "get" => {
            let slot = &state[&target];
            let claim = if live(slot, now) {
                wire(&slot["claim"])
            } else {
                Value::Null
            };
            json!({ "ok": true, "result": { "claim": claim } })
        }
        "list" => {
            let namespace = request["namespace"].as_str().unwrap();
            let prefix = format!("{namespace}/");
            let claims: Vec<Value> = state
                .as_object()
                .unwrap()
                .iter()
                .filter(|(key, slot)| key.starts_with(&prefix) && live(slot, now))
                .map(|(key, slot)| {
                    let mut claim = wire(&slot["claim"]);
                    claim["resource"] = json!(key.strip_prefix(&prefix).unwrap());
                    claim
                })
                .collect();
            json!({ "ok": true, "result": { "claims": claims } })
        }
        "namespaces" => {
            let mut names: Vec<String> = state
                .as_object()
                .unwrap()
                .keys()
                .filter_map(|key| key.split('/').next().map(str::to_owned))
                .collect();
            names.sort();
            names.dedup();
            json!({ "ok": true, "result": { "namespaces": names } })
        }
        "wait" => {
            // The plugin's own blocking: release the lock between looks so a release can
            // land. Hot Sheet issues one `wait` invocation and never loops on a timer.
            let deadline = now + request["timeout_ms"].as_u64().unwrap();
            drop(_lock);
            loop {
                let current: Value = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .unwrap_or_else(|| json!({}));
                let slot = &current[&target];
                if !live(slot, now_ms()) {
                    return json!({ "ok": true, "result": { "free": true } });
                }
                if now_ms() >= deadline {
                    return json!({ "ok": true, "result": {
                        "free": false, "holder": wire(&slot["claim"]),
                    } });
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        }
        "verify_fencing" => {
            let slot = &state[&target];
            let token = request["fencing_token"].as_u64().unwrap();
            let current = slot["fence"].as_u64().unwrap_or(0);
            if current > token {
                err("not_holder", "a later grant exists")
            } else if current == token && live(slot, now) {
                json!({ "ok": true, "result": {} })
            } else if current == token {
                err("expired", "lapsed")
            } else {
                err("invalid", "unknown token")
            }
        }
        other => err("unsupported", other),
    };
    std::fs::write(&path, state.to_string()).unwrap();
    reply
}

// ---- the harness --------------------------------------------------------------------

struct Plugin {
    _dir: tempfile::TempDir,
    home: PathBuf,
    plugin_dir: PathBuf,
    state_dir: PathBuf,
}

fn manifest_text(id: &str, wait: &str, timeout: u64, secret: bool) -> String {
    // HS2-XQHTXC: this binary itself, as a TOML string (a Windows path's backslashes
    // escaped), running only its ignored plugin entry.
    let exe = std::env::current_exe()
        .unwrap()
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!(
        r#"id = "{id}"
display_name = "Fake"
description = "A synthetic claim backend."
command = ["{exe}", "--ignored", "--exact", "fake_plugin_main", "--nocapture"]
timeout_seconds = {timeout}

[capabilities]
guarantee = "exclusive_with_fencing"
wait = "{wait}"
max_metadata_entries = 8

[[settings]]
key = "state_dir"
label = "State directory"
type = "string"
required = true

[[settings]]
key = "style"
label = "Style"
type = "choice"
choices = ["compact", "full"]
default = "compact"

{}"#,
        if secret {
            "[[settings]]\nkey = \"token\"\nlabel = \"Access token\"\ntype = \"secret\"\n"
        } else {
            ""
        },
    )
}

fn install(id: &str, wait: &str, timeout: u64) -> Plugin {
    install_with(id, wait, timeout, true)
}

fn install_with(id: &str, wait: &str, timeout: u64, secret: bool) -> Plugin {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let plugin_dir = home.join("claim-backends").join(id);
    let state_dir = dir.path().join("state");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::write(
        plugin_dir.join("manifest.toml"),
        manifest_text(id, wait, timeout, secret),
    )
    .unwrap();
    Plugin {
        _dir: dir,
        home,
        plugin_dir,
        state_dir,
    }
}

impl Plugin {
    fn configuration(&self, id: &str) -> BackendConfiguration {
        let mut configuration = BackendConfiguration::new("fake", BackendKind::External).unwrap();
        configuration.settings.insert("plugin".into(), id.into());
        configuration
            .settings
            .insert("state_dir".into(), self.state_dir.display().to_string());
        configuration
    }

    fn approved_backend(&self, id: &str) -> Arc<dyn ClaimBackend> {
        approve(&self.home, id, &self.plugin_dir).unwrap();
        self.backend(id).unwrap_or_else(|e| panic!("{e}"))
    }

    /// The backend over an empty synthetic credential store (the native one is denied).
    fn backend(&self, id: &str) -> Result<Arc<dyn ClaimBackend>, ClaimError> {
        external::backend_with(
            &self.home,
            &self.configuration(id),
            SharedSecrets(Arc::new(MemorySecrets::default())),
        )
    }

    fn mode(&self, mode: &str) {
        std::fs::write(self.state_dir.join("mode"), mode).unwrap();
    }

    /// The runs logged so far. HS2-CYDK9Z: only whole lines count; a plugin can still be
    /// appending the last one while a test waits for it (seen on Linux).
    fn runs(&self) -> Vec<Value> {
        std::fs::read_to_string(self.state_dir.join("runs.log"))
            .unwrap_or_default()
            .split_inclusive('\n')
            .filter(|line| line.ends_with('\n'))
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

fn target(text: &str) -> ResourceRef {
    ResourceRef::parse(text).unwrap()
}

fn lease(seconds: u64) -> AcquireRequest {
    AcquireRequest::new(Duration::from_secs(seconds))
}

fn unique_namespace(prefix: &str) -> Namespace {
    Namespace::parse(&format!(
        "{prefix}-{}",
        ulid::Ulid::new().to_string().to_lowercase()
    ))
    .unwrap()
}

// ---- conformance --------------------------------------------------------------------

#[test]
fn hs2_322zsz_an_external_plugin_without_wait_passes_the_conformance_suite() {
    let plugin = install("fake", "none", 20);
    let backend = plugin.approved_backend("fake");
    assert_eq!(backend.capabilities().wait, WaitMechanism::None);
    conformance::run(&Fixture {
        backend: backend.as_ref(),
        namespace: unique_namespace("conf"),
        advance: &std::thread::sleep,
        short_lease: Duration::from_millis(900),
    });
}

#[test]
fn hs2_322zsz_an_external_plugin_with_a_blocking_wait_passes_the_conformance_suite() {
    let plugin = install("fake", "blocking", 20);
    let backend = plugin.approved_backend("fake");
    assert_eq!(backend.capabilities().wait, WaitMechanism::LongPoll);
    conformance::run(&Fixture {
        backend: backend.as_ref(),
        namespace: unique_namespace("conf"),
        advance: &std::thread::sleep,
        short_lease: Duration::from_millis(900),
    });
}

// ---- the protocol -------------------------------------------------------------------

#[test]
fn hs2_322zsz_the_renewal_token_never_reaches_the_plugin_only_its_digest() {
    let plugin = install("fake", "none", 20);
    let backend = plugin.approved_backend("fake");
    let grant = backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    backend
        .renew(grant.renewal_token.expose(), Duration::from_secs(90))
        .unwrap();
    backend.release(grant.renewal_token.expose()).unwrap();
    let runs = plugin.runs();
    assert_eq!(runs.len(), 3);
    for run in &runs {
        let stdin = run["stdin"].as_str().unwrap();
        assert!(!stdin.contains(grant.renewal_token.expose()), "{stdin}");
        assert!(!stdin.contains("hsrc1."), "no token of any shape: {stdin}");
        let envelope: Value = serde_json::from_str(stdin).unwrap();
        assert_eq!(envelope["protocol"], 1);
        assert_eq!(envelope["backend"], "fake");
        assert_eq!(envelope["configuration"]["id"], "fake");
    }
    let digest = runs[1]["stdin"].as_str().unwrap();
    assert!(digest.contains("token_digest"), "{digest}");
}

#[test]
fn hs2_322zsz_secrets_travel_only_on_stdin_never_argv_or_environment() {
    let plugin = install("fake", "none", 20);
    approve(&plugin.home, "fake", &plugin.plugin_dir).unwrap();
    let manifest = PluginManifest::load(&plugin.plugin_dir).unwrap();
    let secret = "synthetic-secret-value-123";
    let settings = json!({
        "state_dir": plugin.state_dir.display().to_string(),
        "token": secret,
    });
    let backend = ExternalBackend::new(
        &plugin.home,
        manifest,
        "fake",
        settings,
        None,
        Arc::new(LiveRunner),
    );
    backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    let run = &plugin.runs()[0];
    assert!(
        run["stdin"].as_str().unwrap().contains(secret),
        "stdin carries it"
    );
    assert!(!run["args"].to_string().contains(secret), "argv must not");
    assert!(
        !run["env"].to_string().contains(secret),
        "the environment must not"
    );
    assert!(
        !format!("{backend:?}").contains(secret),
        "Debug output must not print settings"
    );
}

#[test]
fn hs2_322zsz_a_wait_is_one_blocking_invocation_not_a_loop_of_requests() {
    let plugin = install("fake", "blocking", 20);
    let backend = plugin.approved_backend("fake");
    let held = backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    let before = plugin.runs().len();
    let started = Instant::now();
    let grant = std::thread::scope(|scope| {
        let waiter = scope.spawn(|| {
            backend.acquire_wait(&target("vms/one"), &lease(60), Duration::from_secs(20))
        });
        std::thread::sleep(Duration::from_millis(800));
        backend.release(held.renewal_token.expose()).unwrap();
        waiter.join().unwrap()
    })
    .unwrap();
    assert!(started.elapsed() < Duration::from_secs(15));
    backend.release(grant.renewal_token.expose()).unwrap();
    let ops: Vec<String> = plugin.runs()[before..]
        .iter()
        .map(|run| {
            let envelope: Value = serde_json::from_str(run["stdin"].as_str().unwrap()).unwrap();
            envelope["op"].as_str().unwrap().to_owned()
        })
        .collect();
    let waits = ops.iter().filter(|op| *op == "wait").count();
    assert_eq!(waits, 1, "one wait invocation while parked: {ops:?}");
    assert!(
        ops.iter().filter(|op| *op == "acquire").count() <= 3,
        "no per-tick requests: {ops:?}"
    );
}

// ---- HS2-P234BH: a cancel during the wait ---------------------------------------------

/// A generous bound for "returned promptly" on a loaded machine; the waits below would
/// otherwise last two minutes.
const PROMPT: Duration = Duration::from_secs(5);

impl Plugin {
    fn ops(&self) -> Vec<String> {
        self.runs()
            .iter()
            .map(|run| {
                let envelope: Value = serde_json::from_str(run["stdin"].as_str().unwrap()).unwrap();
                envelope["op"].as_str().unwrap().to_owned()
            })
            .collect()
    }

    /// Block until the plugin has started `count` runs of `op`; the pid of the last one.
    fn until_running(&self, op: &str, count: usize) -> u32 {
        let started = Instant::now();
        loop {
            let runs = self.runs();
            let matching: Vec<&Value> = runs
                .iter()
                .filter(|run| {
                    run["stdin"]
                        .as_str()
                        .unwrap()
                        .contains(&format!("\"op\":\"{op}\""))
                })
                .collect();
            if matching.len() >= count {
                return u32::try_from(matching[count - 1]["pid"].as_u64().unwrap()).unwrap();
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "no {op} run: {:?}",
                self.ops()
            );
            std::thread::yield_now();
        }
    }
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

/// HS2-XQHTXC: whether `pid` is in the process list, through `tasklist` (a local read of
/// the process table, as `kill -0` is on Unix).
#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    let listed = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .stderr(std::process::Stdio::null())
        .output()
        .unwrap();
    String::from_utf8_lossy(&listed.stdout).contains(&format!("\"{pid}\""))
}

fn spawn_plugin_wait(
    backend: &Arc<dyn ClaimBackend>,
    cancel: &WaitCancel,
) -> std::sync::mpsc::Receiver<Result<WaitOutcome, ClaimError>> {
    let (done, result) = std::sync::mpsc::channel();
    let (backend, cancel) = (backend.clone(), cancel.clone());
    std::thread::spawn(move || {
        let _ = done.send(backend.wait_free_cancellable(
            &target("vms/one"),
            Duration::from_secs(120),
            &cancel,
        ));
    });
    result
}

#[test]
fn hs2_p234bh_a_cancel_stops_the_plugin_wait_process_and_nothing_runs_after() {
    let plugin = install("fake", "blocking", 20);
    let backend = plugin.approved_backend("fake");
    let held = backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    let cancel = WaitCancel::new();
    let parked = spawn_plugin_wait(&backend, &cancel);
    let pid = plugin.until_running("wait", 1);
    let before = plugin.ops().len();

    let began = Instant::now();
    cancel.cancel();
    let error = parked.recv_timeout(PROMPT).unwrap().unwrap_err();
    assert!(began.elapsed() < PROMPT, "{:?}", began.elapsed());
    assert_eq!(error.code(), "interrupted");
    // Stopped and reaped like a timed-out operation, not left to finish its wait.
    assert!(!process_alive(pid), "the wait process {pid} was stopped");

    // The release runs; nothing from the cancelled wait runs after it or claims.
    backend.release(held.renewal_token.expose()).unwrap();
    assert_eq!(plugin.ops()[before..], ["release"]);
    assert!(backend.get(&target("vms/one")).unwrap().is_none());

    // Cancelled before it starts (and again): the plugin never runs.
    let before = plugin.ops().len();
    for _ in 0..2 {
        let error = backend
            .wait_free_cancellable(&target("vms/one"), Duration::from_secs(120), &cancel)
            .unwrap_err();
        assert_eq!(error.code(), "interrupted");
    }
    assert_eq!(plugin.ops().len(), before);

    // A fresh signal waits again and a release still wakes it with one invocation.
    let held = backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    let again = spawn_plugin_wait(&backend, &WaitCancel::new());
    plugin.until_running("wait", 2);
    backend.release(held.renewal_token.expose()).unwrap();
    assert_eq!(
        again
            .recv_timeout(Duration::from_secs(20))
            .unwrap()
            .unwrap(),
        WaitOutcome::Free
    );
    assert_eq!(plugin.ops().iter().filter(|op| *op == "wait").count(), 2);
}

// ---- HS2-CYDK9Z: a stop ends the plugin's own children ----------------------------------

impl Plugin {
    /// Block until the plugin has started its own child; that child's pid.
    fn grandchild(&self) -> u32 {
        let path = self.state_dir.join("grandchild.pid");
        let started = Instant::now();
        loop {
            if let Ok(text) = std::fs::read_to_string(&path) {
                return text.trim().parse().unwrap();
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "the plugin started no child"
            );
            std::thread::yield_now();
        }
    }

    /// Forget the last recorded child, for the next run.
    fn forget_grandchild(&self) {
        let _ = std::fs::remove_file(self.state_dir.join("grandchild.pid"));
    }
}

/// Wait until `pid` is gone. A killed child whose parent was killed too is adopted and
/// reaped by the system, so it can stay visible for a moment; this is a local check of a
/// process table entry, not a request.
fn until_gone(pid: u32) -> bool {
    let started = Instant::now();
    while started.elapsed() < PROMPT {
        if !process_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Stop the process if a failed assertion left it running.
struct KillOnDrop(u32);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        #[cfg(unix)]
        let mut command = std::process::Command::new("kill");
        #[cfg(unix)]
        command.args(["-9", &self.0.to_string()]);
        #[cfg(windows)]
        let mut command = std::process::Command::new("taskkill");
        #[cfg(windows)]
        command.args(["/F", "/T", "/PID", &self.0.to_string()]);
        let _ = command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

#[test]
fn hs2_cydk9z_a_timeout_stops_the_plugins_own_children_too() {
    let plugin = install("fake", "none", 1);
    let backend = plugin.approved_backend("fake");
    plugin.mode("forks");
    // Repeated: each run starts a fresh child and each timeout ends it.
    for _ in 0..2 {
        plugin.forget_grandchild();
        let started = Instant::now();
        let error = backend.get(&target("vms/one")).unwrap_err();
        assert_eq!(error.code(), "backend_unreachable");
        assert!(
            error.to_string().contains("did not finish within 1s"),
            "{error}"
        );
        assert!(started.elapsed() < Duration::from_secs(30), "{error}");
        let grandchild = plugin.grandchild();
        let _guard = KillOnDrop(grandchild);
        assert!(
            until_gone(grandchild),
            "the plugin's child {grandchild} outlived the timeout"
        );
    }
    // A plugin that behaves again still works through the same backend.
    plugin.mode("");
    assert!(backend.get(&target("vms/one")).unwrap().is_none());
}

#[test]
fn hs2_cydk9z_a_cancel_stops_the_plugins_own_children_too() {
    let plugin = install("fake", "blocking", 60);
    let backend = plugin.approved_backend("fake");
    let held = backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    plugin.mode("forks");
    for round in 1..=2 {
        plugin.forget_grandchild();
        let cancel = WaitCancel::new();
        let parked = spawn_plugin_wait(&backend, &cancel);
        let pid = plugin.until_running("wait", round);
        let grandchild = plugin.grandchild();
        let _guards = (KillOnDrop(pid), KillOnDrop(grandchild));
        assert!(
            process_alive(grandchild),
            "the child runs before the cancel"
        );

        let began = Instant::now();
        cancel.cancel();
        let error = parked.recv_timeout(PROMPT).unwrap().unwrap_err();
        assert!(began.elapsed() < PROMPT, "{:?}", began.elapsed());
        assert_eq!(error.code(), "interrupted");
        assert!(!process_alive(pid), "the wait process {pid} was stopped");
        assert!(
            until_gone(grandchild),
            "the plugin's child {grandchild} outlived the cancel"
        );
    }
    plugin.mode("");
    backend.release(held.renewal_token.expose()).unwrap();
    assert!(backend.get(&target("vms/one")).unwrap().is_none());
}

// ---- HS2-BR14FM: a plugin that exits normally but leaves a child running --------------

/// What the backend logged while `body` ran on this thread.
fn logged<T>(body: impl FnOnce() -> T) -> (T, String) {
    #[derive(Clone, Default)]
    struct Capture(Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Capture {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
        type Writer = Self;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(capture.clone())
        .with_ansi(false)
        .finish();
    let value = tracing::subscriber::with_default(subscriber, body);
    let text = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    (value, text)
}

const LEFT_RUNNING: &str = "left a process running";

#[test]
fn hs2_br14fm_a_child_holding_the_plugins_output_is_ended_and_the_answer_used() {
    // A long timeout, so only the fix (not the timeout) can end the run promptly.
    let plugin = install("fake", "none", 60);
    let backend = plugin.approved_backend("fake");
    plugin.mode("leaves_child");
    // Repeated: each run leaves a fresh child, and each is ended.
    for round in 1..=2 {
        plugin.forget_grandchild();
        let began = Instant::now();
        let (held, log) = logged(|| backend.acquire(&target(&format!("vms/{round}")), &lease(60)));
        let held = held.unwrap_or_else(|e| panic!("the plugin's answer is used: {e}"));
        assert!(began.elapsed() < PROMPT, "{:?}", began.elapsed());
        assert!(held.claim.fencing_token > 0);
        let grandchild = plugin.grandchild();
        let _guard = KillOnDrop(grandchild);
        assert!(
            until_gone(grandchild),
            "the plugin's child {grandchild} outlived its run"
        );
        assert!(log.contains(LEFT_RUNNING), "{log}");
        assert!(
            log.contains("plugin=fake") && log.contains("op=\"acquire\""),
            "{log}"
        );
        assert!(
            !log.contains(plugin.state_dir.to_str().unwrap()),
            "no settings: {log}"
        );
        // The claim it granted is real.
        let read = backend
            .get(&target(&format!("vms/{round}")))
            .unwrap()
            .unwrap();
        assert_eq!(read.claim_id, held.claim.claim_id);
    }

    // A plugin that stops leaving children is unaffected: no warning, same answers.
    plugin.mode("");
    plugin.forget_grandchild();
    let began = Instant::now();
    let (listed, log) = logged(|| backend.get(&target("vms/1")));
    assert!(listed.unwrap().is_some());
    assert!(began.elapsed() < PROMPT, "{:?}", began.elapsed());
    assert!(!log.contains(LEFT_RUNNING), "{log}");
}

#[test]
fn hs2_br14fm_a_quiet_leftover_is_ended_without_a_warning() {
    let plugin = install("fake", "none", 60);
    let backend = plugin.approved_backend("fake");
    plugin.mode("leaves_quiet_child");
    let (read, log) = logged(|| backend.get(&target("vms/one")));
    assert!(read.unwrap().is_none());
    let grandchild = plugin.grandchild();
    let _guard = KillOnDrop(grandchild);
    // Its tree is ended as a stop ends it, though nothing held the output.
    assert!(
        until_gone(grandchild),
        "the quiet child {grandchild} outlived its run"
    );
    // HS2-03PD7G: on Windows a process the plugin starts inherits the plugin's inheritable
    // handles, its output pipe included, even with null stdio, so there the "quiet" child
    // really holds the output and the warning is accurate. It is still ended above.
    if cfg!(not(windows)) {
        assert!(!log.contains(LEFT_RUNNING), "{log}");
    }
}

// Unix only: the leftovers are started by `/bin/sh` scripts (`&`, `$!`, redirections). The
// same behaviour runs on every platform through the leaves-child plugin tests above.
#[cfg(unix)]
#[test]
fn hs2_br14fm_the_runner_reports_leftovers_only_when_they_hold_the_output() {
    use hotsheet_resource_claims::external::CommandRunner;
    let run = |script: &str| {
        let began = Instant::now();
        let output = LiveRunner
            .run(
                "/bin/sh",
                &["-c".to_owned(), script.to_owned()],
                b"",
                Duration::from_secs(60),
            )
            .unwrap();
        assert!(began.elapsed() < PROMPT, "{script}: {:?}", began.elapsed());
        output
    };
    // Exits at once, holding nothing: unaffected.
    let plain = run("echo answer; echo note >&2");
    assert!(plain.success && !plain.left_running);
    assert_eq!(String::from_utf8_lossy(&plain.stdout), "answer\n");
    assert_eq!(String::from_utf8_lossy(&plain.stderr), "note\n");
    // A child holding stdout, then stderr only: the output so far is kept, the child ended.
    for redirect in ["2>/dev/null", ">/dev/null"] {
        let held = run(&format!(
            "sleep 120 {redirect} </dev/null & echo $!; echo answer; exit 4"
        ));
        assert!(held.left_running, "{redirect}");
        assert_eq!(held.code, Some(4));
        let text = String::from_utf8_lossy(&held.stdout).into_owned();
        let mut lines = text.lines();
        let pid: u32 = lines.next().unwrap().parse().unwrap();
        let _guard = KillOnDrop(pid);
        assert_eq!(lines.next(), Some("answer"));
        assert!(until_gone(pid), "the child {pid} outlived its run");
    }
}

// ---- approval -----------------------------------------------------------------------

#[test]
fn hs2_322zsz_an_unapproved_plugin_is_refused_before_it_runs() {
    let plugin = install("fake", "none", 20);
    let refused = plugin
        .backend("fake")
        .err()
        .expect("an unapproved plugin is refused");
    assert_eq!(refused.code(), "unsupported");
    assert!(
        refused.to_string().contains("resource plugin trust fake"),
        "{refused}"
    );
    assert!(plugin.runs().is_empty(), "the plugin never ran");
}

#[test]
fn hs2_322zsz_an_edited_plugin_is_refused_on_the_next_operation_until_trusted_again() {
    let plugin = install("fake", "none", 20);
    let backend = plugin.approved_backend("fake");
    backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    let runs = plugin.runs().len();
    std::fs::write(plugin.plugin_dir.join("helper.sh"), "echo changed\n").unwrap();
    let refused = backend.get(&target("vms/one")).unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("changed since it was approved"),
        "{refused}"
    );
    assert_eq!(plugin.runs().len(), runs, "edited code never runs");
    // A new backend over the same directory is refused too, and trusting re-opens it.
    assert!(plugin.backend("fake").is_err());
    approve(&plugin.home, "fake", &plugin.plugin_dir).unwrap();
    assert!(backend.get(&target("vms/one")).unwrap().is_some());
}

#[test]
fn hs2_322zsz_approval_is_idempotent_and_a_repeated_add_never_retrusts_changed_code() {
    use hotsheet_resource_claims::external::approve_if_new;
    use hotsheet_ticketing::claim_backends::Change;
    let plugin = install("fake", "none", 20);
    assert!(approve_if_new(&plugin.home, "fake", &plugin.plugin_dir).unwrap());
    assert_eq!(
        approve(&plugin.home, "fake", &plugin.plugin_dir).unwrap(),
        Change::Unchanged
    );
    std::fs::write(plugin.plugin_dir.join("extra"), "x").unwrap();
    assert!(
        !approve_if_new(&plugin.home, "fake", &plugin.plugin_dir).unwrap(),
        "an existing approval is kept, not refreshed"
    );
    assert!(
        external::approval_state(&plugin.home, "fake", &plugin.plugin_dir)
            .is_ok_and(|s| !s.is_approved())
    );
    assert_eq!(
        approve(&plugin.home, "fake", &plugin.plugin_dir).unwrap(),
        Change::Updated
    );
}

// ---- failures -----------------------------------------------------------------------

#[test]
fn hs2_322zsz_a_plugin_that_outlives_its_timeout_is_stopped() {
    let plugin = install("fake", "none", 1);
    let backend = plugin.approved_backend("fake");
    plugin.mode("hang");
    let started = Instant::now();
    let error = backend.get(&target("vms/one")).unwrap_err();
    assert_eq!(error.code(), "backend_unreachable");
    assert!(
        error.to_string().contains("did not finish within 1s"),
        "{error}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "killed, not awaited"
    );
}

#[test]
fn hs2_322zsz_malformed_crashed_and_lying_plugins_fail_without_granting() {
    let plugin = install("fake", "none", 20);
    let backend = plugin.approved_backend("fake");
    for (mode, code, needle) in [
        ("garbage", "backend_unreachable", "without a result"),
        ("crash", "backend_unreachable", "exited with 3"),
        (
            "ok_but_failed_exit",
            "backend_unreachable",
            "reported success but exited",
        ),
        ("fencing_zero", "invalid", "fencing_token above 0"),
        ("wrong_claim", "invalid", "other than the one requested"),
    ] {
        plugin.mode(mode);
        // A lying plugin may leave its claim behind, so each mode has its own resource.
        let error = backend
            .acquire(
                &target(&format!("vms/{}", mode.replace('_', "-"))),
                &lease(60),
            )
            .expect_err(mode);
        assert_eq!(error.code(), code, "{mode}: {error}");
        assert!(error.to_string().contains(needle), "{mode}: {error}");
    }
}

#[test]
fn hs2_322zsz_a_held_resource_conflict_names_the_holder_on_the_requested_target() {
    let plugin = install("fake", "none", 20);
    let backend = plugin.approved_backend("fake");
    let first = backend.acquire(&target("vms/one"), &lease(60)).unwrap();
    match backend.acquire(&target("vms/one"), &lease(60)) {
        Err(ClaimError::HeldBy { holder }) => {
            assert_eq!(holder.target(), "vms/one");
            assert_eq!(holder.claim_id, first.claim.claim_id);
        }
        other => panic!("{other:?}"),
    }
}

// ---- manifests and configuration ----------------------------------------------------

fn write_manifest(root: &Path, dir: &str, text: &str) {
    let path = root.join(dir);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("manifest.toml"), text).unwrap();
}

const MINIMAL: &str = r#"
command = ["run.sh"]
[capabilities]
guarantee = "advisory_only"
"#;

#[test]
fn hs2_322zsz_load_errors_are_listed_without_hiding_the_other_plugins() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_manifest(
        root,
        "good",
        &format!("id = \"good\"\ndisplay_name = \"Good\"{MINIMAL}"),
    );
    write_manifest(
        root,
        "local",
        &format!("id = \"local\"\ndisplay_name = \"X\"{MINIMAL}"),
    );
    write_manifest(
        root,
        "renamed",
        &format!("id = \"other\"\ndisplay_name = \"X\"{MINIMAL}"),
    );
    write_manifest(root, "broken", "this is not toml");
    write_manifest(
        root,
        "nocmd",
        "id = \"nocmd\"\ndisplay_name = \"X\"\ncommand = []\n[capabilities]\nguarantee = \"advisory_only\"\n",
    );
    std::fs::create_dir_all(root.join("empty")).unwrap();
    let (plugins, errors) = load_all(root);
    assert_eq!(
        plugins.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        ["good"]
    );
    let text = errors.join("\n");
    assert!(text.contains("built-in backend and always wins"), "{text}");
    assert!(text.contains("must match its directory name"), "{text}");
    assert!(text.contains("command must name a program"), "{text}");
    assert!(text.contains("no manifest.toml"), "{text}");
    assert_eq!(errors.len(), 5, "{text}");
}

#[test]
fn hs2_322zsz_settings_are_checked_against_the_manifest_and_secrets_stay_out_of_files() {
    let plugin = install("fake", "none", 20);
    let manifest = PluginManifest::load(&plugin.plugin_dir).unwrap();
    let mut configuration = plugin.configuration("fake");
    external::check_settings(&manifest, &configuration).unwrap();
    for (key, value, needle) in [
        (
            "token",
            "pasted",
            "is a secret: never put it in a configuration",
        ),
        ("style", "huge", "must be one of compact, full"),
        ("nope", "x", "has no setting 'nope'"),
    ] {
        let mut bad = configuration.clone();
        bad.settings.insert(key.into(), value.into());
        let error = external::check_settings(&manifest, &bad).unwrap_err();
        assert!(error.to_string().contains(needle), "{key}: {error}");
        assert!(
            !error.to_string().contains(value) || key == "nope",
            "{key}: never echoes a value"
        );
    }
    configuration.settings.remove("state_dir");
    let missing = external::check_settings(&manifest, &configuration).unwrap_err();
    assert!(
        missing.to_string().contains("needs setting 'state_dir'"),
        "{missing}"
    );
}

#[test]
fn hs2_322zsz_a_bound_namespace_resolves_to_the_plugin_through_the_shared_resolver() {
    use hotsheet_ticketing::claim_backends::Scope;
    // No secret setting: the shared resolver would read the (denied) native store.
    let plugin = install_with("fake", "none", 20, false);
    Scope::Machine {
        home: plugin.home.clone(),
    }
    .update(|r| {
        r.upsert_configuration(plugin.configuration("fake"))?;
        r.bind_namespace("vms", "fake");
        Ok(())
    })
    .unwrap();
    approve(&plugin.home, "fake", &plugin.plugin_dir).unwrap();
    let resolved = resolve(&plugin.home, None, &Namespace::parse("vms").unwrap()).unwrap();
    assert_eq!(resolved.configuration.as_deref(), Some("fake"));
    assert_eq!(resolved.offline, OfflinePolicy::FailClosed);
    let grant = resolved
        .backend
        .acquire(&target("vms/one"), &lease(60))
        .unwrap();
    assert_eq!(
        resolved
            .backend
            .get(&target("vms/one"))
            .unwrap()
            .unwrap()
            .claim_id,
        grant.claim.claim_id
    );
    // An unbound namespace still goes to the machine's local backend.
    let other = resolve(&plugin.home, None, &Namespace::parse("other").unwrap()).unwrap();
    assert_eq!(other.configuration, None);
}
