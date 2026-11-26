//! HS2-Z0VYZ5: an AI completion of a provider-backed ticket must have its commits on the
//! remote, exactly as a Git-store completion (HS2-R8ZH4K). The code lives in the checkout
//! that links the provider connection, not in the provider. Every server path is covered:
//! the provider-scoped update and close routes, the checkout-scoped update and close
//! routes, and the MCP shim's `hotsheet_update`/`hotsheet_close` over a real socket. The
//! providers are fakes with real shapes: a GitHub provider over a stateful in-process
//! GitHub transport (`acme/repo#N` slugs) and a Git provider relabelled as a Jira-style
//! connection (`PROJ-…` keys). The remote is a real temporary bare repository. Synthetic
//! data only.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hotsheet_extsync::{GitHubConfig, GitHubProvider, GitHubTransport, HttpResponse};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{FsStore, GitProvider, NewTicket, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const SECRET: &str = "synthetic-secret";

fn reply(status: u16, body: Value) -> HttpResponse {
    HttpResponse {
        status,
        headers: HashMap::new(),
        body: body.to_string(),
    }
}

/// A stateful GitHub Issues fake with GitHub's response shapes: issues, their comments
/// (listed, created), and PATCH moving `state`/`state_reason`/labels.
#[derive(Default)]
struct FakeGitHub {
    issues: Mutex<BTreeMap<u64, Value>>,
    comments: Mutex<BTreeMap<u64, Vec<Value>>>,
    clock: Mutex<u64>,
}

impl FakeGitHub {
    fn seed(&self, number: u64) {
        self.issues.lock().unwrap().insert(
            number,
            json!({
                "number": number, "node_id": format!("I_fake{number}"),
                "title": format!("Synthetic issue {number}"), "body": "Synthetic.",
                "state": "open", "state_reason": null,
                "html_url": format!("https://github.com/acme/repo/issues/{number}"),
                "created_at": "2026-11-01T00:00:00Z", "updated_at": "2026-11-01T00:01:00Z",
                "closed_at": null, "labels": [{ "name": "status:started" }],
                "assignees": [], "pull_request": null
            }),
        );
    }

    fn state(&self, number: u64) -> String {
        self.issues.lock().unwrap()[&number]["state"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn comment_bodies(&self, number: u64) -> Vec<String> {
        self.comments
            .lock()
            .unwrap()
            .get(&number)
            .into_iter()
            .flatten()
            .map(|comment| comment["body"].as_str().unwrap().to_owned())
            .collect()
    }

    fn tick(&self) -> String {
        let mut clock = self.clock.lock().unwrap();
        *clock += 1;
        format!("2026-11-26T00:{:02}:{:02}Z", *clock / 60, *clock % 60)
    }
}

impl GitHubTransport for FakeGitHub {
    fn request(
        &self,
        method: &str,
        url: &str,
        _: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<HttpResponse, String> {
        let (path, query) = url.split_once('?').unwrap_or((url, ""));
        let rest = path
            .split("/repos/acme/repo/issues")
            .nth(1)
            .unwrap_or("-")
            .trim_start_matches('/');
        if rest.is_empty() {
            let issues = self.issues.lock().unwrap();
            return Ok(reply(200, json!(issues.values().collect::<Vec<_>>())));
        }
        let mut parts = rest.split('/');
        let Ok(number) = parts.next().unwrap_or("-").parse::<u64>() else {
            return Ok(reply(404, json!({ "message": "Not Found" })));
        };
        if !self.issues.lock().unwrap().contains_key(&number) {
            return Ok(reply(404, json!({ "message": "Not Found" })));
        }
        match (method, parts.next()) {
            ("GET", Some("comments")) => {
                let page_one = !query.contains("page=")
                    || query.contains("page=1&")
                    || query.ends_with("page=1");
                let comments = if page_one {
                    self.comments
                        .lock()
                        .unwrap()
                        .get(&number)
                        .cloned()
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                Ok(reply(200, json!(comments)))
            }
            ("POST", Some("comments")) => {
                let now = self.tick();
                let mut comments = self.comments.lock().unwrap();
                let list = comments.entry(number).or_default();
                let comment = json!({
                    "id": 1000 + number * 100 + list.len() as u64,
                    "body": body.unwrap()["body"].clone(),
                    "created_at": now, "updated_at": now,
                    "user": { "login": "synthetic-bot" }
                });
                list.push(comment.clone());
                Ok(reply(201, comment))
            }
            ("GET", Some(_)) => Ok(reply(200, json!([]))),
            ("GET", None) => Ok(reply(200, self.issues.lock().unwrap()[&number].clone())),
            ("PATCH", None) => {
                let now = self.tick();
                let body = body.unwrap();
                let mut issues = self.issues.lock().unwrap();
                let issue = issues.get_mut(&number).unwrap();
                if let Some(labels) = body.get("labels").and_then(Value::as_array) {
                    issue["labels"] = json!(
                        labels
                            .iter()
                            .map(|name| json!({ "name": name }))
                            .collect::<Vec<_>>()
                    );
                }
                match body.get("state").and_then(Value::as_str) {
                    Some("closed") => {
                        issue["state"] = json!("closed");
                        issue["closed_at"] = json!(now);
                        issue["state_reason"] = body
                            .get("state_reason")
                            .cloned()
                            .unwrap_or(json!("completed"));
                    }
                    Some("open") => {
                        issue["state"] = json!("open");
                        issue["state_reason"] = json!("reopened");
                        issue["closed_at"] = Value::Null;
                    }
                    _ => {}
                }
                issue["updated_at"] = json!(now);
                Ok(reply(200, issue.clone()))
            }
            _ => Ok(reply(404, json!({ "message": "Not Found" }))),
        }
    }
}

/// Run git in `dir` with a fixed identity; returns stdout.
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
    /// The code checkout: a clone of a bare remote, linking both provider connections.
    code: PathBuf,
    router: axum::Router,
    state: AppState,
    registry: PathBuf,
    checkout: String,
    github: Arc<FakeGitHub>,
    jira: FsStore,
}

impl Project {
    fn commit(&self, file: &str, subject: &str) -> String {
        std::fs::write(self.code.join(file), subject).unwrap();
        git(&self.code, &["add", file]);
        git(&self.code, &["commit", "--quiet", "-m", subject]);
        git(&self.code, &["rev-parse", "HEAD"]).trim().to_owned()
    }

    fn push(&self, refspec: &str) {
        git(&self.code, &["push", "--quiet", "origin", refspec]);
    }

    /// A Jira-style ticket in the relabelled Git provider; returns (native id, key).
    fn jira_ticket(&self, title: &str) -> (String, String) {
        let ticket = hotsheet_ticketing::ops::create(
            &self.jira,
            hotsheet_model::Ulid::new(),
            "PROJ",
            hotsheet_model::Timestamp::new("2026-11-26T00:00:00Z"),
            NewTicket {
                title: title.into(),
                ..Default::default()
            },
        )
        .unwrap();
        (ticket.id.to_string(), ticket.slug)
    }

    fn jira_status(&self, native: &str) -> hotsheet_model::Status {
        self.jira
            .read_ticket(&hotsheet_model::Ulid::from_string(native).unwrap())
            .unwrap()
            .status
    }
}

/// A checkout (a clone of a bare remote) linking one Git store, a GitHub connection, and a
/// Jira-style connection.
fn project() -> Project {
    let dir = tempfile::tempdir().unwrap();
    let remote = dir.path().join("remote.git");
    std::fs::create_dir(&remote).unwrap();
    git(&remote, &["init", "--quiet", "--bare"]);
    git(
        dir.path(),
        &["clone", "--quiet", remote.to_str().unwrap(), "code"],
    );
    let code = dir.path().join("code");
    std::fs::write(code.join("base.txt"), "base").unwrap();
    git(&code, &["add", "."]);
    git(&code, &["commit", "--quiet", "-m", "base"]);
    git(&code, &["push", "--quiet", "origin", "HEAD:main"]);
    let store_root = dir.path().join("AA.hs2");
    FsStore::init_git(&store_root, &StoreMetadata::new("AA")).unwrap();
    let registry = dir.path().join("checkouts.json");
    let registered = CheckoutRegistry::new(&registry)
        .register(&code, Some("project"), None, vec![store_root.clone()])
        .unwrap();
    let mut saved: Value = serde_json::from_slice(&std::fs::read(&registry).unwrap()).unwrap();
    let sources = saved["checkouts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["id"] == registered.id)
        .unwrap()["sources"]
        .as_array_mut()
        .unwrap();
    sources.push(
        json!({ "connection_id": "github-main", "provider": "github", "locator": "acme/repo" }),
    );
    sources.push(json!({ "connection_id": "jira-main", "provider": "jira", "locator": "PROJ" }));
    std::fs::write(&registry, serde_json::to_vec(&saved).unwrap()).unwrap();
    let github = Arc::new(FakeGitHub::default());
    let mut config = GitHubConfig::new("github-main", "acme/repo", "fixture-token");
    config.api_base = "https://api.test".into();
    let jira = FsStore::init_git(dir.path().join("jira"), &StoreMetadata::new("PROJ")).unwrap();
    let state = AppState::new(FsStore::open(&store_root).unwrap(), SECRET.into())
        .unwrap()
        .with_default_codex_home(dir.path().join("no-codex-home"))
        .with_checkout_registry(&registry)
        .with_ticket_provider(Arc::new(GitHubProvider::new(config, github.clone())))
        .with_ticket_provider(Arc::new(GitProvider::new("jira-main", jira.clone())));
    Project {
        _dir: dir,
        code,
        router: app(state.clone()),
        state,
        registry,
        checkout: registered.id,
        github,
        jira,
    }
}

async fn send(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-hotsheet-secret", SECRET)
        .header("content-type", "application/json")
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn ai() -> Value {
    json!({ "role": "ai", "id": "worker" })
}

/// A completing update body for an AI actor, with a scored note, plus `extra` fields.
fn complete(extra: Value) -> Value {
    let mut body = json!({
        "status": "completed", "note": "## Confidence\n85", "note_confidence": 85,
        "actor": ai()
    });
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    body
}

fn refused(status: StatusCode, body: &Value, code: &str) {
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], code, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("Nothing was changed"),
        "{body}"
    );
}

fn has_opt_out_note(ticket: &Value) -> bool {
    ticket["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note["text"].as_str().unwrap().contains("no-commit opt-out"))
}

/// The provider-scoped update route: a GitHub issue's `#N` commits must be on the remote.
#[tokio::test]
async fn hs2_z0vyz5_provider_update_requires_github_commits_on_the_remote() {
    let project = project();
    project.github.seed(7);
    let router = &project.router;
    let uri = "/providers/github-main/tickets/7";

    let (status, body) = send(router, "PATCH", uri, Some(complete(json!({})))).await;
    refused(status, &body, "commits_not_found");
    git(&project.code, &["checkout", "--quiet", "-b", "issue-7"]);
    let sha = project.commit("work.txt", "#7 fix the synthetic bug");
    let (status, body) = send(router, "PATCH", uri, Some(complete(json!({})))).await;
    refused(status, &body, "commit_not_on_remote");
    let message = body["error"].as_str().unwrap();
    assert!(message.contains(&sha[..12]), "{message}");
    assert!(
        message.contains("git push origin issue-7:merge-pending/acme/repo#7"),
        "{message}"
    );
    assert_eq!(project.github.state(7), "open", "a refusal changes nothing");
    assert!(project.github.comment_bodies(7).is_empty());

    project.push("issue-7:merge-pending/acme/repo#7");
    let (status, body) = send(router, "PATCH", uri, Some(complete(json!({})))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "completed", "{body}");
    assert_eq!(project.github.state(7), "closed");
    assert!(!has_opt_out_note(&body));
}

/// The provider-scoped close route: the no-commit opt-out is recorded as a GitHub
/// comment; a human close is never checked; commits combined with the opt-out are invalid.
#[tokio::test]
async fn hs2_z0vyz5_provider_close_records_the_opt_out_through_the_provider() {
    let project = project();
    project.github.seed(8);
    project.github.seed(9);
    let router = &project.router;
    // Score the cycle first: a close carries no note of its own.
    let (status, body) = send(
        router,
        "PATCH",
        "/providers/github-main/tickets/8",
        Some(json!({ "note": "## Confidence\n90", "note_confidence": 90, "actor": ai() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let close = "/providers/github-main/tickets/8/close";
    let (status, body) = send(
        router,
        "POST",
        close,
        Some(json!({ "reason": "completed", "actor": ai() })),
    )
    .await;
    refused(status, &body, "commits_not_found");
    let (status, body) = send(
        router,
        "POST",
        close,
        Some(
            json!({ "reason": "completed", "actor": ai(), "no_commit": true,
                     "commits": ["HEAD"] }),
        ),
    )
    .await;
    refused(status, &body, "commit_invalid");
    assert_eq!(project.github.state(8), "open");
    let (status, body) = send(
        router,
        "POST",
        close,
        Some(json!({ "reason": "completed", "actor": ai(), "no_commit": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(project.github.state(8), "closed");
    assert!(has_opt_out_note(&body), "{body}");
    assert!(
        project
            .github
            .comment_bodies(8)
            .iter()
            .any(|comment| comment.contains("no-commit opt-out")),
        "the opt-out is a GitHub comment"
    );

    // A human completes with nothing pushed: the rule does not apply.
    project.commit("local.txt", "#9 local only");
    let (status, body) = send(
        router,
        "POST",
        "/providers/github-main/tickets/9/close",
        Some(json!({ "reason": "completed", "actor": { "role": "human" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(project.github.state(9), "closed");
    assert!(!has_opt_out_note(&body));
}

/// The checkout-scoped routes for a Jira-style connection: the route's checkout is the code
/// checkout; the close route applies the confidence rule and the remote rule (it called the
/// provider directly before HS2-Z0VYZ5); explicit commits are verified.
#[tokio::test]
async fn hs2_z0vyz5_checkout_routes_check_provider_completions() {
    let project = project();
    let router = &project.router;
    let checkout = format!("/checkouts/{}", project.checkout);

    let (native, key) = project.jira_ticket("Closed through the checkout");
    let close = format!("{checkout}/tickets/jira-main:{native}/close");
    let ai_close = json!({ "reason": "completed", "actor": ai() });
    let (status, body) = send(router, "POST", &close, Some(ai_close.clone())).await;
    refused(status, &body, "confidence_required");
    let (status, body) = send(
        router,
        "PATCH",
        &format!("{checkout}/tickets/jira-main:{native}"),
        Some(json!({ "note": "## Confidence\n80", "note_confidence": 80, "actor": ai() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    git(&project.code, &["checkout", "--quiet", "-b", "jira-work"]);
    project.commit("jira.txt", &format!("{key} the Jira work"));
    let (status, body) = send(router, "POST", &close, Some(ai_close.clone())).await;
    refused(status, &body, "commit_not_on_remote");
    assert_ne!(
        project.jira_status(&native),
        hotsheet_model::Status::Completed
    );
    project.push(&format!("jira-work:merge-pending/{key}"));
    let (status, body) = send(router, "POST", &close, Some(ai_close)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["store"], "jira-main");
    assert_eq!(
        project.jira_status(&native),
        hotsheet_model::Status::Completed
    );

    // The checkout update route with explicit commits: an unknown SHA is invalid, a local
    // one is refused, a pushed one passes even though its subject names no key.
    let (native, _) = project.jira_ticket("Updated through the checkout");
    let update = format!("{checkout}/tickets/jira-main:{native}");
    let (status, body) = send(
        router,
        "PATCH",
        &update,
        Some(complete(
            json!({ "commits": ["0123456789abcdef0123456789abcdef01234567"] }),
        )),
    )
    .await;
    refused(status, &body, "commit_invalid");
    let sha = project.commit("named.txt", "work without a key in the subject");
    let named = complete(json!({ "commits": [sha] }));
    let (status, body) = send(router, "PATCH", &update, Some(named.clone())).await;
    refused(status, &body, "commit_not_on_remote");
    project.push("jira-work:main");
    let (status, body) = send(router, "PATCH", &update, Some(named)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "completed", "{body}");

    // A human completion through the checkout is never checked.
    let (native, key) = project.jira_ticket("Human work");
    project.commit("human.txt", &format!("{key} local only"));
    let (status, body) = send(
        router,
        "PATCH",
        &format!("{checkout}/tickets/jira-main:{native}"),
        Some(json!({ "status": "completed", "actor": { "role": "human" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        project.jira_status(&native),
        hotsheet_model::Status::Completed
    );
}

/// With no checkout linking the connection, the completion passes with a warning, the
/// same as a Git store with no known checkout.
#[tokio::test]
async fn hs2_z0vyz5_unlinked_connection_warns_instead_of_checking() {
    let project = project();
    let registry = project.registry.clone();
    let mut saved: Value = serde_json::from_slice(&std::fs::read(&registry).unwrap()).unwrap();
    saved["checkouts"][0]["sources"]
        .as_array_mut()
        .unwrap()
        .retain(|source| source["connection_id"] != "jira-main");
    std::fs::write(&registry, serde_json::to_vec(&saved).unwrap()).unwrap();
    let (native, _) = project.jira_ticket("No code checkout");
    let (status, body) = send(
        &project.router,
        "PATCH",
        &format!("/providers/jira-main/tickets/{native}"),
        Some(complete(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning
                .as_str()
                .unwrap()
                .contains("remote-commit check skipped")),
        "{body}"
    );
}

/// The MCP shim's `hotsheet_update`/`hotsheet_close` with `connection` reach the provider
/// routes over a real socket, carrying `commits` and `no_commit`.
#[tokio::test]
async fn hs2_z0vyz5_mcp_provider_completions_carry_the_commit_claim() {
    let project = project();
    project.github.seed(11);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let state = project.state.clone();
    tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });
    let (native, key) = project.jira_ticket("Closed through MCP");
    project.commit("mcp.txt", &format!("{key} local only"));
    let project = Arc::new(project);
    let native_for_mcp = native.clone();
    let shared = project.clone();
    tokio::task::spawn_blocking(move || {
        let project = shared;
        let native = native_for_mcp;
        let backend = hotsheet_mcp::HttpBackend::new(&base, SECRET);
        let call = |name: &str, arguments: Value| {
            let reply = hotsheet_mcp::handle_message(
                &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                        "params":{"name": name, "arguments": arguments}}),
                &backend,
            )
            .unwrap();
            let text = reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .to_owned();
            (reply["result"]["isError"] == true, text)
        };
        let (error, text) = call(
            "hotsheet_update",
            json!({ "connection": "github-main", "id": "11", "status": "completed",
                    "note": "## Confidence\n90", "note_confidence": 90,
                    "actor_role": "ai", "actor_id": "worker", "no_commit": true }),
        );
        assert!(!error, "{text}");
        assert!(text.contains("no-commit opt-out"), "{text}");
        assert_eq!(project.github.state(11), "closed");

        let (error, text) = call(
            "hotsheet_update",
            json!({ "connection": "jira-main", "id": native, "note": "## Confidence\n90",
                    "note_confidence": 90, "actor_role": "ai", "actor_id": "worker" }),
        );
        assert!(!error, "{text}");
        let close = json!({ "connection": "jira-main", "id": native, "reason": "completed",
                            "actor_role": "ai", "actor_id": "worker" });
        let (error, text) = call("hotsheet_close", close.clone());
        assert!(error && text.contains("commit_not_on_remote"), "{text}");
        let mut opted = close;
        opted["no_commit"] = json!(true);
        let (error, text) = call("hotsheet_close", opted);
        assert!(!error, "{text}");
        assert!(text.contains("no-commit opt-out"), "{text}");
    })
    .await
    .unwrap();
    assert_eq!(
        project.jira_status(&native),
        hotsheet_model::Status::Completed
    );
}
