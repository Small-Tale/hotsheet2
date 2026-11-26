//! HS2-Z0VYZ5: `provider-edit` and `provider-close` apply the remote-commit completion
//! guard (HS2-R8ZH4K) to provider-backed tickets through the built `hotsheet-cli` binary.
//! The code checkout is the registered checkout linking the connection: a real clone of a
//! temporary bare remote. Covered: the store's own Git connection, and a GitHub connection
//! served by a local stateful GitHub fake (`acme/repo#N` slugs, `#N` commit subjects).
//! Local-only commits are refused, pushed ones accepted, `--no-commit` is recorded, and a
//! human actor is never checked. Isolated homes, synthetic data only.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn git(dir: &Path, args: &[&str]) -> String {
    let output = hotsheet_ticketing::git::command()
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=T", "-c", "user.email=t@example.invalid"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

struct Project {
    _dir: tempfile::TempDir,
    home: PathBuf,
    store: PathBuf,
    code: PathBuf,
}

impl Project {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let store = dir.path().join("store");
        let remote = dir.path().join("remote.git");
        for path in [&home, &store, &remote] {
            std::fs::create_dir_all(path).unwrap();
        }
        git(&remote, &["init", "--quiet", "--bare"]);
        git(
            dir.path(),
            &["clone", "--quiet", remote.to_str().unwrap(), "code"],
        );
        let code = dir.path().join("code");
        let project = Self {
            _dir: dir,
            home,
            store,
            code,
        };
        project.commit("base.txt", "base");
        project.push("HEAD:main");
        project
            .cli()
            .args(["init", "--prefix", "HS"])
            .assert()
            .success();
        project
            .cli()
            .args(["checkout", "register"])
            .arg(&project.code)
            .arg("--store")
            .arg(&project.store)
            .assert()
            .success();
        project
    }

    /// The built CLI against the store, with an isolated home and no ambient actor.
    fn cli(&self) -> Command {
        let mut cmd = Command::new(workspace_bin::path("hotsheet-cli"));
        cmd.arg("--test-fixture-credentials")
            .env("HOTSHEET_HOME", &self.home)
            .env("HOTSHEET_NATIVE_SECRETS", "deny")
            .env_remove("HOTSHEET_ACTOR_ROLE")
            .env_remove("HOTSHEET_ACTOR_ID")
            .env_remove("HOTSHEET_STORE")
            .arg("-C")
            .arg(&self.store);
        cmd
    }

    fn ai(&self) -> Command {
        let mut cmd = self.cli();
        cmd.args(["--actor-role", "ai", "--actor-id", "worker"]);
        cmd
    }

    fn commit(&self, file: &str, subject: &str) -> String {
        std::fs::write(self.code.join(file), subject).unwrap();
        git(&self.code, &["add", file]);
        git(&self.code, &["commit", "--quiet", "-m", subject]);
        git(&self.code, &["rev-parse", "HEAD"]).trim().to_owned()
    }

    fn push(&self, refspec: &str) {
        git(&self.code, &["push", "--quiet", "origin", refspec]);
    }

    fn store(&self) -> hotsheet_ticketing::FsStore {
        hotsheet_ticketing::FsStore::open(&self.store).unwrap()
    }

    /// A new store ticket; returns (slug, native ULID).
    fn ticket(&self, title: &str) -> (String, String) {
        let out = self
            .cli()
            .args(["new", "--title", title])
            .assert()
            .success();
        let slug = String::from_utf8(out.get_output().stdout.clone())
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_owned();
        let native = hotsheet_ticketing::ops::resolve(&self.store(), &slug)
            .unwrap()
            .unwrap()
            .id
            .to_string();
        (slug, native)
    }

    fn status(&self, native: &str) -> hotsheet_model::Status {
        self.store()
            .read_ticket(&hotsheet_model::Ulid::from_string(native).unwrap())
            .unwrap()
            .status
    }
}

const SCORED: [&str; 4] = ["--note", "## Confidence\n85", "--note-confidence", "85"];

/// The store's own Git connection through `provider-edit` and `provider-close`.
#[test]
fn hs2_z0vyz5_provider_commands_require_commits_on_the_remote() {
    let project = Project::new();
    let connection = hotsheet_ticketing::provider::git_connection_id(&project.store());

    // provider-edit: none, local only, pushed.
    let (slug, native) = project.ticket("Coded through the provider");
    let edit = || {
        let mut cmd = project.ai();
        cmd.args([
            "provider-edit",
            &connection,
            &native,
            "--status",
            "completed",
        ])
        .args(SCORED);
        cmd
    };
    edit()
        .assert()
        .failure()
        .stderr(predicate::str::contains("commits_not_found"));
    git(
        &project.code,
        &["checkout", "--quiet", "-b", "worker-branch"],
    );
    let sha = project.commit("work.txt", &format!("{slug} the work"));
    edit()
        .assert()
        .failure()
        .stderr(predicate::str::contains("commit_not_on_remote"))
        .stderr(predicate::str::contains(&sha[..12]))
        .stderr(predicate::str::contains(format!(
            "git push origin worker-branch:merge-pending/{slug}"
        )));
    assert_eq!(project.status(&native), hotsheet_model::Status::NotStarted);
    project.push(&format!("worker-branch:merge-pending/{slug}"));
    edit().assert().success();
    assert_eq!(project.status(&native), hotsheet_model::Status::Completed);

    // provider-close: refused without commits, recorded with --no-commit.
    let (_, native) = project.ticket("Decision through the provider");
    project
        .ai()
        .args(["provider-edit", &connection, &native])
        .args(SCORED)
        .assert()
        .success();
    let close = || {
        let mut cmd = project.ai();
        cmd.args([
            "provider-close",
            &connection,
            &native,
            "--reason",
            "completed",
        ]);
        cmd
    };
    close()
        .assert()
        .failure()
        .stderr(predicate::str::contains("commits_not_found"));
    close()
        .args(["--no-commit", "--commit", "HEAD"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
    close()
        .arg("--no-commit")
        .assert()
        .success()
        .stdout(predicate::str::contains("Completed without code commits"));
    assert_eq!(project.status(&native), hotsheet_model::Status::Completed);

    // provider-edit --no-commit records the opt-out too.
    let (_, native) = project.ticket("Investigation through the provider");
    project
        .ai()
        .args([
            "provider-edit",
            &connection,
            &native,
            "--status",
            "completed",
        ])
        .args(SCORED)
        .arg("--no-commit")
        .assert()
        .success()
        .stdout(predicate::str::contains("Completed without code commits"));

    // provider-close applies the confidence rule before the remote rule.
    let (slug, native) = project.ticket("Unscored");
    project.commit("unscored.txt", &format!("{slug} local only"));
    close_unscored(&project, &connection, &native);

    // A human completes with nothing pushed: the rule does not apply.
    let (slug, native) = project.ticket("Human work");
    project.commit("human.txt", &format!("{slug} local only"));
    project
        .cli()
        .args(["--actor-role", "human"])
        .args([
            "provider-close",
            &connection,
            &native,
            "--reason",
            "completed",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Completed without code commits").not());
    assert_eq!(project.status(&native), hotsheet_model::Status::Completed);
}

fn close_unscored(project: &Project, connection: &str, native: &str) {
    project
        .ai()
        .args([
            "provider-close",
            connection,
            native,
            "--reason",
            "completed",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("confidence_required"));
}

/// A stateful GitHub Issues fake on a local socket: issues, comments, and PATCH.
#[derive(Default)]
struct FakeGitHub {
    issues: Mutex<BTreeMap<u64, Value>>,
    comments: Mutex<BTreeMap<u64, Vec<Value>>>,
}

impl FakeGitHub {
    fn seed(&self, number: u64) {
        self.issues.lock().unwrap().insert(
            number,
            json!({
                "number": number, "node_id": format!("I_fake{number}"),
                "title": format!("Synthetic issue {number}"), "body": "Synthetic.",
                "state": "open", "state_reason": null,
                "html_url": format!("https://github.test/acme/repo/issues/{number}"),
                "created_at": "2026-11-01T00:00:00Z", "updated_at": "2026-11-01T00:01:00Z",
                "closed_at": null, "labels": [], "assignees": [], "pull_request": null
            }),
        );
    }

    fn state(&self, number: u64) -> String {
        self.issues.lock().unwrap()[&number]["state"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn answer(&self, method: &str, target: &str, body: Option<Value>) -> (u16, Value) {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let rest = path.strip_prefix("/repos/acme/repo/issues/").unwrap_or("-");
        let mut parts = rest.split('/');
        let Ok(number) = parts.next().unwrap_or("-").parse::<u64>() else {
            return (404, json!({ "message": "Not Found" }));
        };
        let mut issues = self.issues.lock().unwrap();
        let Some(issue) = issues.get_mut(&number) else {
            return (404, json!({ "message": "Not Found" }));
        };
        match (method, parts.next()) {
            ("GET", None) => (200, issue.clone()),
            ("GET", Some("comments")) => {
                let first = !query.contains("page=")
                    || query.contains("page=1&")
                    || query.ends_with("page=1");
                let comments = self.comments.lock().unwrap();
                let list = comments.get(&number).cloned().unwrap_or_default();
                (200, if first { json!(list) } else { json!([]) })
            }
            ("POST", Some("comments")) => {
                let mut comments = self.comments.lock().unwrap();
                let list = comments.entry(number).or_default();
                let comment = json!({
                    "id": 1000 + list.len() as u64, "body": body.unwrap()["body"].clone(),
                    "created_at": "2026-11-26T00:00:00Z", "updated_at": "2026-11-26T00:00:00Z",
                    "user": { "login": "synthetic-bot" }
                });
                list.push(comment.clone());
                (201, comment)
            }
            ("GET", Some(_)) => (200, json!([])),
            ("PATCH", None) => {
                let body = body.unwrap();
                if let Some(labels) = body.get("labels").and_then(Value::as_array) {
                    issue["labels"] = json!(
                        labels
                            .iter()
                            .map(|name| json!({ "name": name }))
                            .collect::<Vec<_>>()
                    );
                }
                if let Some(state) = body.get("state") {
                    issue["state"] = state.clone();
                    issue["state_reason"] = body
                        .get("state_reason")
                        .cloned()
                        .unwrap_or(json!("completed"));
                }
                (200, issue.clone())
            }
            _ => (404, json!({ "message": "Not Found" })),
        }
    }

    /// Serve forever on a background thread; returns the base URL.
    fn serve(self: &Arc<Self>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let fake = self.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let (method, target, body) = read_request(&mut stream);
                let (status, reply) = fake.answer(&method, &target, body);
                let reply = reply.to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
            }
        });
        base
    }
}

/// One HTTP/1.1 request: (method, target, JSON body).
fn read_request(stream: &mut std::net::TcpStream) -> (String, String, Option<Value>) {
    stream.set_nonblocking(false).unwrap();
    let mut request = Vec::new();
    let mut buf = [0; 4096];
    loop {
        if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&request[..end]).into_owned();
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            if request.len() >= end + 4 + length {
                let mut first = headers.lines().next().unwrap().split_whitespace();
                let method = first.next().unwrap().to_owned();
                let target = first.next().unwrap().to_owned();
                let body = (length > 0)
                    .then(|| serde_json::from_slice(&request[end + 4..end + 4 + length]).ok())
                    .flatten();
                return (method, target, body);
            }
        }
        let read = stream.read(&mut buf).unwrap();
        assert!(read > 0, "client closed before a complete request");
        request.extend_from_slice(&buf[..read]);
    }
}

/// A GitHub connection linked to the code checkout: `#N` commits must be on the remote.
#[test]
fn hs2_z0vyz5_github_provider_edit_requires_issue_commits_on_the_remote() {
    let project = Project::new();
    let github = Arc::new(FakeGitHub::default());
    github.seed(7);
    let base = github.serve();
    std::fs::write(
        project.store.join("providers.json"),
        json!({ "connections": [{
            "id": "github-main", "provider": "github", "locator": "acme/repo",
            "settings": { "api_base": base, "credential": { "secret": "cli-github-fixture" } }
        }] })
        .to_string(),
    )
    .unwrap();
    // Link the connection to the checkout, as `provider connect` does.
    let registry = project.home.join("checkouts.json");
    let mut saved: Value = serde_json::from_slice(&std::fs::read(&registry).unwrap()).unwrap();
    saved["checkouts"][0]["sources"]
        .as_array_mut()
        .unwrap()
        .push(
            json!({ "connection_id": "github-main", "provider": "github",
                      "locator": "acme/repo" }),
        );
    std::fs::write(&registry, saved.to_string()).unwrap();
    let edit = || {
        let mut cmd = project.ai();
        cmd.env("HOTSHEET_API_KEY_CLI_GITHUB_FIXTURE", "fixture-token")
            .args(["provider-edit", "github-main", "7", "--status", "completed"])
            .args(SCORED);
        cmd
    };
    git(&project.code, &["checkout", "--quiet", "-b", "issue-7"]);
    let sha = project.commit("issue.txt", "#7 fix the synthetic issue");
    edit()
        .assert()
        .failure()
        .stderr(predicate::str::contains("commit_not_on_remote"))
        .stderr(predicate::str::contains(&sha[..12]));
    assert_eq!(github.state(7), "open", "a refusal changes nothing");
    project.push("issue-7:main");
    edit().assert().success();
    assert_eq!(github.state(7), "closed");
}
