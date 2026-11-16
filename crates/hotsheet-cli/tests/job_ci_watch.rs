//! HS2-3R9V0R end to end: the built `hotsheet-cli` watches a commit's CI gate through its
//! real HTTP client against a loopback fixture server. Nothing reaches the network, and
//! the token is synthetic. The fixture answers with the REST shapes, ETags and status
//! codes of the provider's endpoints.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use hotsheet_jobs::{ChangeWaiter, Job, JobStore, MonitorSlot, Status};
use serde_json::{Value, json};

const SHA: &str = "1111111111111111111111111111111111111111";
const TOKEN: &str = "synthetic-token-for-tests";
const RUNS: &str = "/repos/acme/widgets/actions/runs?branch=main&event=push&per_page=30";
const JOBS: &str = "/repos/acme/widgets/actions/runs/9/jobs?per_page=100";
const DEADLINE: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
struct Seen {
    method: String,
    path: String,
    status: u16,
    authorization: Option<String>,
    accept: Option<String>,
}

#[derive(Default)]
struct Provider {
    routes: BTreeMap<String, (String, String)>,
    versions: u64,
    seen: Vec<Seen>,
}

/// A minimal HTTP/1.1 server on a loopback port.
#[derive(Clone)]
struct Fake {
    state: Arc<Mutex<Provider>>,
    answered: Arc<Condvar>,
    url: String,
}

impl Fake {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(Provider::default()));
        let shared = state.clone();
        let answered = Arc::new(Condvar::new());
        let notify = answered.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                let mut headers = BTreeMap::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
                    }
                }
                let mut parts = request_line.split_whitespace();
                let (method, path) = (
                    parts.next().unwrap_or("").to_owned(),
                    parts.next().unwrap_or("").to_owned(),
                );
                let mut provider = shared.lock().unwrap();
                let authorized = headers.get("authorization").map(String::as_str)
                    == Some(&format!("Bearer {TOKEN}"));
                let (status, etag, body) = match provider.routes.get(&path) {
                    _ if !authorized => {
                        (401, None, json!({"message": "Bad credentials"}).to_string())
                    }
                    Some((etag, _)) if headers.get("if-none-match") == Some(etag) => {
                        (304, Some(etag.clone()), String::new())
                    }
                    Some((etag, body)) => (200, Some(etag.clone()), body.clone()),
                    None => (404, None, json!({"message": "Not Found"}).to_string()),
                };
                provider.seen.push(Seen {
                    method,
                    path,
                    status,
                    authorization: headers.get("authorization").cloned(),
                    accept: headers.get("accept").cloned(),
                });
                drop(provider);
                notify.notify_all();
                let reason = match status {
                    200 => "OK",
                    304 => "Not Modified",
                    401 => "Unauthorized",
                    _ => "Not Found",
                };
                let etag = etag.map_or(String::new(), |etag| format!("ETag: {etag}\r\n"));
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n{etag}\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Self {
            state,
            answered,
            url,
        }
    }

    /// Block until the provider has answered a request that `check` accepts.
    fn until_seen(&self, what: &str, check: impl Fn(&Seen) -> bool) {
        let guard = self.state.lock().unwrap();
        let (_guard, timeout) = self
            .answered
            .wait_timeout_while(guard, DEADLINE, |provider| {
                !provider.seen.iter().any(&check)
            })
            .unwrap();
        assert!(!timeout.timed_out(), "timed out waiting for {what}");
    }

    fn serve(&self, path: &str, body: Value) {
        let mut provider = self.state.lock().unwrap();
        provider.versions += 1;
        let etag = format!("W/\"v{}\"", provider.versions);
        provider
            .routes
            .insert(path.to_owned(), (etag, body.to_string()));
    }

    fn run(&self, status: &str, conclusion: Option<&str>) {
        self.serve(
            RUNS,
            json!({"total_count": 1, "workflow_runs": [{
                "id": 9, "name": "CI", "head_branch": "main", "head_sha": SHA, "event": "push",
                "status": status, "conclusion": conclusion, "run_attempt": 1,
                "html_url": "https://github.example.test/acme/widgets/actions/runs/9",
                "created_at": "2026-11-01T10:09:00Z", "updated_at": "2026-11-01T10:09:30Z",
                "run_started_at": "2026-11-01T10:09:00Z"
            }]}),
        );
    }

    fn seen(&self) -> Vec<Seen> {
        self.state.lock().unwrap().seen.clone()
    }
}

struct Fixture {
    home: tempfile::TempDir,
    checkout: tempfile::TempDir,
    checkout_id: String,
    store: JobStore,
    provider: Fake,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let checkout = tempfile::tempdir().unwrap();
        Self {
            checkout_id: hotsheet_ticketing::checkouts::checkout_id(checkout.path()).unwrap(),
            store: JobStore::at(home.path()),
            provider: Fake::start(),
            home,
            checkout,
        }
    }

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(workspace_bin::path("hotsheet-cli"))
            .args(args)
            .env("HOTSHEET_HOME", self.home.path())
            .env("HOTSHEET_GITHUB_API_URL", &self.provider.url)
            .env("GH_TOKEN", TOKEN)
            .env_remove("GITHUB_TOKEN")
            .env_remove("HOTSHEET_ACTOR_ROLE")
            .env_remove("HOTSHEET_ACTOR_ID")
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.cli(args);
        assert!(
            output.status.success(),
            "{args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn target_args(&self) -> Vec<String> {
        vec![
            "--commit".into(),
            SHA.into(),
            "--repo".into(),
            "acme/widgets".into(),
            "--checkout".into(),
            self.checkout.path().to_string_lossy().into_owned(),
        ]
    }

    fn watch(&self, extra: &[&str]) -> Value {
        let target = self.target_args();
        let mut args = vec!["job", "watch", "ci"];
        args.extend(target.iter().map(String::as_str));
        args.extend_from_slice(extra);
        args.extend_from_slice(&["--poll-ms", "40", "--idle-exit-ms", "500"]);
        self.json(&args)
    }

    fn gate(&self, extra: &[&str]) -> Value {
        let target = self.target_args();
        let mut args = vec!["job", "watch", "gate"];
        args.extend(target.iter().map(String::as_str));
        args.extend_from_slice(extra);
        self.json(&args)
    }

    fn until_job(&self, id: &str, what: &str, check: impl Fn(&Job) -> bool) -> Job {
        let waiter = ChangeWaiter::new(MonitorSlot::at(self.home.path()).dir()).unwrap();
        let deadline = Instant::now() + DEADLINE;
        loop {
            let job = self.store.get(&self.checkout_id, id).unwrap();
            if check(&job) {
                return job;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            waiter.wait(Duration::from_millis(50));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.cli(&["job", "watch", "stop"]);
    }
}

#[test]
fn a_commit_is_watched_to_a_green_gate_through_the_real_http_client() {
    let fx = Fixture::new();
    assert_eq!(
        fx.gate(&[])["state"],
        json!("unknown"),
        "nothing is watched yet"
    );

    fx.provider.run("in_progress", None);
    fx.provider.serve(
        JOBS,
        json!({"total_count": 2, "jobs": [
            {"id": 91, "name": "build", "status": "completed", "conclusion": "success",
             "steps": [{"name": "compile", "status": "completed", "number": 1}]},
            {"id": 92, "name": "test", "status": "in_progress", "conclusion": null,
             "steps": [{"name": "unit", "status": "completed", "number": 1},
                       {"name": "browser", "status": "in_progress", "number": 2}]}
        ]}),
    );
    let ticket = "c5c4d57ccef3d9ba:01ARZ3NDEKTSV4RRFFQ69G5FAV";
    let started = fx.watch(&["--ticket", ticket]);
    assert_eq!(started["checkout_id"], json!(fx.checkout_id));
    assert_eq!(started["monitor"]["running"], json!(true));
    assert_eq!(started["job"]["title"], json!("CI on main for 1111111"));
    assert_eq!(
        started["job"]["ticket"]["native_id"],
        json!("01ARZ3NDEKTSV4RRFFQ69G5FAV")
    );
    assert_eq!(started["job"]["capabilities"]["cancel"], json!(true));
    let job_id = started["job"]["id"].as_str().unwrap().to_owned();

    // Repeating the command finds the same Job.
    let again = fx.watch(&["--ticket", ticket]);
    assert_eq!(again["job"]["id"], json!(job_id));
    assert_eq!(again["monitor"]["started"], json!(false));

    let running = fx.until_job(&job_id, "the run to be reported", |job| {
        job.status == Status::Running && job.steps.len() == 2
    });
    assert_eq!(running.steps[1].label, "test");
    assert_eq!(
        (running.steps[1].done, running.steps[1].total),
        (Some(1), Some(2))
    );
    assert_eq!(running.latest["run_id"], "9");
    assert_eq!(
        running.url.as_deref(),
        Some("https://github.example.test/acme/widgets/actions/runs/9")
    );
    let pending = fx.gate(&[]);
    assert_eq!(pending["state"], json!("pending"));
    assert_eq!(pending["job_id"], json!(job_id));

    // Unchanged answers come back as 304 to the conditional requests the client sends.
    fx.provider
        .until_seen("a conditional request answered 304", |seen| {
            seen.status == 304
        });

    fx.provider.run("completed", Some("success"));
    // The reader blocks on the store and returns as soon as the verdict is written.
    let green = fx.gate(&["--wait-seconds", "30"]);
    assert_eq!(green["state"], json!("green"));
    assert_eq!(green["run"]["id"], json!(9));
    assert_eq!(green["run"]["head_sha"], json!(SHA));
    assert_eq!(
        green["run"]["url"],
        json!("https://github.example.test/acme/widgets/actions/runs/9")
    );
    let done = fx.store.get(&fx.checkout_id, &job_id).unwrap();
    assert_eq!(done.status, Status::Succeeded);
    let outcome = done.outcome.unwrap();
    assert_eq!(
        outcome.summary.as_deref(),
        Some("CI run 9 passed for 1111111")
    );
    assert_eq!(outcome.refs["commit"], SHA);

    // Every request carried the credential and the API media type, and none was a write.
    let seen = fx.provider.seen();
    assert!(seen.iter().all(|request| {
        request.authorization.as_deref() == Some(&format!("Bearer {TOKEN}"))
            && request.accept.as_deref() == Some("application/vnd.github+json")
            && request.method == "GET"
    }));
    assert!(
        seen.iter()
            .any(|request| request.path == RUNS && request.status == 200)
    );
    assert!(seen.iter().any(|request| request.path == JOBS));

    // The token never reaches the machine-local records.
    for entry in walk(fx.home.path()) {
        let bytes = std::fs::read(&entry).unwrap_or_default();
        assert!(
            !String::from_utf8_lossy(&bytes).contains(TOKEN),
            "the token was written to {}",
            entry.display()
        );
    }

    // A green gate is final: asking again starts nothing and keeps the Job.
    let after = fx.watch(&["--rewatch"]);
    assert_eq!(after["job"]["id"], json!(job_id));
    assert_eq!(after["gate"]["state"], json!("green"));
}

#[test]
fn bad_targets_are_refused_with_next_steps_and_create_nothing() {
    let fx = Fixture::new();
    let checkout = fx.checkout.path().to_string_lossy().into_owned();
    let short = fx.cli(&[
        "job",
        "watch",
        "ci",
        "--commit",
        "abc123",
        "--repo",
        "acme/widgets",
        "--checkout",
        &checkout,
    ]);
    assert!(!short.status.success());
    assert!(String::from_utf8_lossy(&short.stderr).contains("full 40-character SHA"));

    // The temporary checkout has no `origin` remote to read a repository from.
    let no_repo = fx.cli(&[
        "job",
        "watch",
        "ci",
        "--commit",
        SHA,
        "--checkout",
        &checkout,
    ]);
    assert!(!no_repo.status.success());
    assert!(String::from_utf8_lossy(&no_repo.stderr).contains("--repo <owner>/<name>"));

    let bad_ticket = fx.cli(&[
        "job",
        "watch",
        "ci",
        "--commit",
        SHA,
        "--repo",
        "acme/widgets",
        "--checkout",
        &checkout,
        "--ticket",
        "no-colon",
    ]);
    assert!(!bad_ticket.status.success());
    assert!(String::from_utf8_lossy(&bad_ticket.stderr).contains("<connection_id>:<native_id>"));

    assert!(!fx.home.path().join("jobs").exists());
    assert!(fx.provider.seen().is_empty());
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}
