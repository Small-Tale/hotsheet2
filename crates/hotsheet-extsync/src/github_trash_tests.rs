//! GH-5: Delete moves a GitHub issue to Trash, Restore brings back its exact prior state,
//! and expiry or Empty Trash deletes it only when GitHub allows it.

use super::super::{GitHubConfig, GitHubProvider, GitHubTransport, HttpResponse};
use super::*;
use hotsheet_model::Timestamp;
use hotsheet_ticketing::{ProviderPatch, ProviderTrashPurge, TicketProvider, purge_provider_trash};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

/// How the fake answers `deleteIssue`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DeleteAccess {
    /// A repository admin: the issue is deleted.
    Admin,
    /// A write-only user token: GraphQL 200 with a FORBIDDEN error, as GitHub answers.
    UserWithoutAdmin,
    /// A GitHub App installation token: REST-style 403.
    Integration,
}

/// A stateful GitHub Issues fake following the REST and GraphQL response shapes: PATCH
/// replaces labels and moves `closed_at`/`state_reason`, a deleted issue answers 410, and
/// lists leave it out.
struct FakeGitHub {
    issues: Mutex<BTreeMap<u64, Value>>,
    deleted: Mutex<Vec<u64>>,
    access: Mutex<DeleteAccess>,
    clock: Mutex<u64>,
    requests: Mutex<Vec<(String, String, Option<Value>)>>,
}

impl FakeGitHub {
    fn new(access: DeleteAccess) -> Arc<Self> {
        Arc::new(Self {
            issues: Mutex::default(),
            deleted: Mutex::default(),
            access: Mutex::new(access),
            clock: Mutex::new(0),
            requests: Mutex::default(),
        })
    }

    fn tick(&self) -> String {
        let mut clock = self.clock.lock().unwrap();
        *clock += 1;
        format!("2026-11-20T00:{:02}:{:02}Z", *clock / 60, *clock % 60)
    }

    fn seed(&self, number: u64, state: &str, state_reason: Option<&str>, labels: &[&str]) {
        let closed = state == "closed";
        self.issues.lock().unwrap().insert(
            number,
            json!({
                "number": number,
                "node_id": format!("I_kwDOfake{number}"),
                "title": format!("Synthetic issue {number}"),
                "body": "Synthetic details.",
                "state": state,
                "state_reason": state_reason,
                "html_url": format!("https://github.com/acme/widgets/issues/{number}"),
                "created_at": "2026-11-01T00:00:00Z",
                "updated_at": "2026-11-01T00:01:00Z",
                "closed_at": closed.then_some("2026-11-01T00:01:00Z"),
                "labels": labels.iter().map(|name| json!({"name": name, "color": "ededed"})).collect::<Vec<_>>(),
                "assignees": [],
                "pull_request": null
            }),
        );
    }

    fn set_updated_at(&self, number: u64, at: &str) {
        self.issues.lock().unwrap().get_mut(&number).unwrap()["updated_at"] = json!(at);
    }

    fn labels(&self, number: u64) -> Vec<String> {
        let issues = self.issues.lock().unwrap();
        let mut names = issues[&number]["labels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|label| label["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn field(&self, number: u64, name: &str) -> Value {
        self.issues.lock().unwrap()[&number][name].clone()
    }

    fn exists(&self, number: u64) -> bool {
        self.issues.lock().unwrap().contains_key(&number)
    }

    fn graphql_deletes(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, url, _)| method == "POST" && url.ends_with("/graphql"))
            .count()
    }

    fn reply(status: u16, body: Value) -> HttpResponse {
        HttpResponse {
            status,
            headers: HashMap::new(),
            body: body.to_string(),
        }
    }

    fn patch(&self, number: u64, body: &Value) -> HttpResponse {
        let now = self.tick();
        let mut issues = self.issues.lock().unwrap();
        let Some(issue) = issues.get_mut(&number) else {
            return Self::reply(410, json!({"message": "This issue was deleted"}));
        };
        if let Some(labels) = body.get("labels").and_then(Value::as_array) {
            issue["labels"] = json!(
                labels
                    .iter()
                    .map(|name| json!({"name": name, "color": "ededed"}))
                    .collect::<Vec<_>>()
            );
        }
        for field in ["title", "body"] {
            if let Some(value) = body.get(field) {
                issue[field] = value.clone();
            }
        }
        match body.get("state").and_then(Value::as_str) {
            Some("closed") => {
                if issue["state"] != "closed" {
                    issue["closed_at"] = json!(now);
                }
                issue["state"] = json!("closed");
                // GitHub resets an omitted reason to `completed`.
                issue["state_reason"] = body
                    .get("state_reason")
                    .cloned()
                    .unwrap_or(json!("completed"));
            }
            Some("open") => {
                if issue["state"] == "closed" {
                    issue["state_reason"] = json!("reopened");
                }
                issue["state"] = json!("open");
                issue["closed_at"] = Value::Null;
            }
            _ => {}
        }
        issue["updated_at"] = json!(now);
        Self::reply(200, issue.clone())
    }

    fn graphql(&self, body: &Value) -> HttpResponse {
        let node = body["variables"]["id"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        match *self.access.lock().unwrap() {
            DeleteAccess::Integration => {
                return Self::reply(
                    403,
                    json!({"message": "Resource not accessible by integration",
                           "documentation_url": "https://docs.github.com/graphql"}),
                );
            }
            DeleteAccess::UserWithoutAdmin => {
                return Self::reply(
                    200,
                    json!({
                        "data": {"deleteIssue": null},
                        "errors": [{
                            "type": "FORBIDDEN",
                            "path": ["deleteIssue"],
                            "locations": [{"line": 1, "column": 19}],
                            "message": "octocat does not have the correct permissions to execute `DeleteIssue`"
                        }]
                    }),
                );
            }
            DeleteAccess::Admin => {}
        }
        let mut issues = self.issues.lock().unwrap();
        let found = issues
            .iter()
            .find(|(_, issue)| issue["node_id"] == node)
            .map(|(number, _)| *number);
        match found {
            Some(number) => {
                issues.remove(&number);
                self.deleted.lock().unwrap().push(number);
                Self::reply(
                    200,
                    json!({"data": {"deleteIssue": {"clientMutationId": null}}}),
                )
            }
            None => Self::reply(
                200,
                json!({
                    "data": {"deleteIssue": null},
                    "errors": [{"type": "NOT_FOUND", "path": ["deleteIssue"],
                                "message": format!("Could not resolve to a node with the global id of '{node}'")}]
                }),
            ),
        }
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
        self.requests
            .lock()
            .unwrap()
            .push((method.into(), url.into(), body.cloned()));
        let path = url.split('?').next().unwrap_or(url);
        if method == "POST" && path.ends_with("/graphql") {
            return Ok(self.graphql(body.unwrap_or(&Value::Null)));
        }
        let rest = path
            .strip_prefix("https://api.test/repos/acme/widgets/issues")
            .unwrap_or_default();
        if rest.is_empty() && method == "GET" {
            let issues = self.issues.lock().unwrap();
            return Ok(Self::reply(200, json!(issues.values().collect::<Vec<_>>())));
        }
        if method == "GET"
            && (rest.ends_with("/comments")
                || rest.ends_with("/events")
                || rest.ends_with("/timeline"))
        {
            return Ok(Self::reply(200, json!([])));
        }
        let Ok(number) = rest.trim_start_matches('/').parse::<u64>() else {
            return Ok(Self::reply(404, json!({"message": "Not Found"})));
        };
        if self.deleted.lock().unwrap().contains(&number) {
            return Ok(Self::reply(
                410,
                json!({"message": "This issue was deleted"}),
            ));
        }
        match method {
            "GET" => Ok(match self.issues.lock().unwrap().get(&number) {
                Some(issue) => Self::reply(200, issue.clone()),
                None => Self::reply(404, json!({"message": "Not Found"})),
            }),
            "PATCH" => Ok(self.patch(number, body.unwrap_or(&Value::Null))),
            _ => Ok(Self::reply(404, json!({"message": "Not Found"}))),
        }
    }
}

fn provider(fake: &Arc<FakeGitHub>) -> GitHubProvider {
    let mut config = GitHubConfig::new("github-main", "acme/widgets", "test-token");
    config.api_base = "https://api.test".into();
    GitHubProvider::new(config, fake.clone())
}

fn now() -> Timestamp {
    Timestamp::new("2026-11-25T00:00:00Z")
}

fn delete(github: &GitHubProvider, number: &str) -> ApiTicket {
    github
        .update(
            number,
            now(),
            ProviderPatch {
                status: Some(Status::Deleted),
                ..Default::default()
            },
        )
        .unwrap()
}

/// A starting Hot Sheet status, GitHub's `state` and `state_reason`, the labels Hot Sheet
/// wrote, and the close reason the ticket reads back with.
type StartingState = (
    Status,
    &'static str,
    Option<&'static str>,
    Vec<&'static str>,
    Option<CloseReason>,
);

/// Each starting state, with GitHub's own fields and the labels Hot Sheet wrote.
fn starting_states() -> Vec<StartingState> {
    vec![
        (
            Status::NotStarted,
            "open",
            None,
            vec!["category:bug", "priority:high", "up-next", "customer"],
            None,
        ),
        (
            Status::Started,
            "open",
            None,
            vec!["category:bug", "priority:high", "status:started"],
            None,
        ),
        (
            Status::Backlog,
            "open",
            None,
            vec!["category:bug", "priority:default", "status:backlog"],
            None,
        ),
        (
            Status::Completed,
            "closed",
            Some("completed"),
            vec!["category:bug", "priority:high"],
            Some(CloseReason::Completed),
        ),
        (
            Status::Completed,
            "closed",
            Some("not_planned"),
            vec!["category:bug", "priority:high"],
            Some(CloseReason::NotPlanned),
        ),
        (
            Status::Verified,
            "closed",
            Some("completed"),
            vec!["category:bug", "priority:high", "status:verified"],
            Some(CloseReason::Completed),
        ),
        (
            Status::Verified,
            "closed",
            Some("not_planned"),
            vec![
                "category:bug",
                "priority:high",
                "status:verified",
                "closed:works-as-designed",
            ],
            Some(CloseReason::WorksAsDesigned),
        ),
        (
            Status::Archive,
            "closed",
            Some("completed"),
            vec!["category:bug", "priority:high", "status:archived"],
            Some(CloseReason::Completed),
        ),
    ]
}

#[test]
fn gh_5_delete_trashes_every_status_and_restore_returns_it_exactly() {
    for (status, state, reason, labels, close_reason) in starting_states() {
        let fake = FakeGitHub::new(DeleteAccess::Admin);
        fake.seed(5, state, reason, &labels);
        let github = provider(&fake);
        let before = github.get("5").unwrap();
        assert_eq!(before.status, status, "{labels:?}");

        // Delete: closed as not planned, in Trash, never Completed (the reported bug).
        let trashed = delete(&github, "5");
        assert_eq!(trashed.status, Status::Deleted, "{status:?}");
        assert_eq!(trashed.close_reason, close_reason, "{status:?}");
        assert!(!trashed.up_next);
        assert_eq!(trashed.tags, before.tags, "ordinary tags survive Delete");
        assert_eq!(fake.field(5, "state"), "closed");
        assert_eq!(fake.field(5, "state_reason"), "not_planned");
        let stored = fake.labels(5);
        assert!(stored.contains(&"status:deleted".to_string()), "{stored:?}");
        assert!(
            stored.contains(&format!("deleted-from:{}", status_name(status).unwrap())),
            "{stored:?}"
        );
        assert!(!stored.contains(&"up-next".to_string()));
        assert!(
            !stored.contains(&"status:verified".to_string()),
            "{stored:?}"
        );

        // A repeated Delete is idempotent: the same labels, the same restore target.
        let again = delete(&github, "5");
        assert_eq!(again.status, Status::Deleted);
        assert_eq!(fake.labels(5), stored, "{status:?}");

        // The board's Trash query finds it; the default list hides it from active views.
        let trash = github
            .query(&hotsheet_ticketing::TicketQuery {
                status: Some(Status::Deleted),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(trash.len(), 1);

        // Restore brings back the status, GitHub state, reason, and tags; no Trash label.
        let restored = github.restore("5", now()).unwrap();
        assert_eq!(restored.status, status, "{labels:?}");
        assert_eq!(restored.close_reason, close_reason, "{labels:?}");
        assert_eq!(restored.tags, before.tags);
        assert_eq!(fake.field(5, "state"), state);
        if let Some(reason) = reason {
            assert_eq!(fake.field(5, "state_reason"), reason, "{labels:?}");
        }
        let after = fake.labels(5);
        assert!(
            after.iter().all(|label| label != "status:deleted"
                && !label.starts_with("deleted-from:")
                && label != "closed:completed"
                && label != "closed:not-planned"),
            "{after:?}"
        );

        // Restoring again is a NotInTrash refusal with no write.
        let writes = fake.requests.lock().unwrap().len();
        assert!(matches!(
            github.restore("5", now()),
            Err(ProviderError::Operation(OpError::NotInTrash(found))) if found == status
        ));
        assert_eq!(
            fake.requests.lock().unwrap().len(),
            writes + 1,
            "one read, no write"
        );
    }
}

#[test]
fn gh_5_a_trashed_issue_without_a_restore_label_reopens_as_not_started() {
    let fake = FakeGitHub::new(DeleteAccess::Admin);
    // Trashed outside Hot Sheet: someone added the label by hand.
    fake.seed(
        7,
        "closed",
        Some("not_planned"),
        &["category:task", "status:deleted"],
    );
    let github = provider(&fake);
    let ticket = github.get("7").unwrap();
    assert_eq!(ticket.status, Status::Deleted);
    assert_eq!(ticket.close_reason, None);
    let restored = github.restore("7", now()).unwrap();
    assert_eq!(restored.status, Status::NotStarted);
    assert_eq!(fake.field(7, "state"), "open");
}

#[test]
fn gh_5_reopening_a_trashed_issue_on_github_takes_it_out_of_trash() {
    let fake = FakeGitHub::new(DeleteAccess::Admin);
    fake.seed(
        8,
        "open",
        None,
        &["category:task", "status:deleted", "deleted-from:verified"],
    );
    let ticket = provider(&fake).get("8").unwrap();
    assert_eq!(
        ticket.status,
        Status::NotStarted,
        "a stale closed-only label is ignored"
    );
    assert!(ticket.tags.is_empty(), "Trash labels are never tags");
}

#[test]
fn gh_5_purge_deletes_only_trashed_issues_and_is_idempotent() {
    let fake = FakeGitHub::new(DeleteAccess::Admin);
    fake.seed(
        1,
        "closed",
        Some("completed"),
        &["category:bug", "status:verified"],
    );
    fake.seed(2, "open", None, &["category:bug"]);
    let github = provider(&fake);
    delete(&github, "1");

    // An issue outside Trash is refused before any GraphQL write.
    assert!(matches!(
        github.purge_trashed("2"),
        Err(ProviderError::Operation(OpError::NotInTrash(
            Status::NotStarted
        )))
    ));
    assert_eq!(fake.graphql_deletes(), 0);

    assert_eq!(
        github.purge_trashed("1").unwrap(),
        ProviderPurgeOutcome::Purged
    );
    assert!(!fake.exists(1));
    let request = fake
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|(_, url, _)| url.ends_with("/graphql"))
        .cloned()
        .unwrap();
    assert_eq!(request.1, "https://api.test/graphql");
    assert_eq!(request.2.unwrap()["variables"]["id"], "I_kwDOfake1");

    // Repeats: GitHub answers 410 Gone for the deleted issue, which still counts as purged.
    assert_eq!(
        github.purge_trashed("1").unwrap(),
        ProviderPurgeOutcome::Purged
    );
    assert_eq!(fake.graphql_deletes(), 1, "no second delete");
    assert!(matches!(
        github.get("1"),
        Err(ProviderError::NotFound { .. })
    ));
}

#[test]
fn gh_5_purge_without_permission_keeps_the_issue_trashed_and_says_why() {
    for access in [DeleteAccess::UserWithoutAdmin, DeleteAccess::Integration] {
        let fake = FakeGitHub::new(access);
        fake.seed(
            3,
            "closed",
            Some("completed"),
            &["category:bug", "status:verified"],
        );
        let github = provider(&fake);
        delete(&github, "3");
        let outcome = github.purge_trashed("3").unwrap();
        let ProviderPurgeOutcome::Retained { message } = outcome else {
            panic!("a refused delete must not read as purged");
        };
        assert!(message.starts_with(PURGE_NOT_PERMITTED), "{message}");
        assert!(fake.exists(3));
        assert_eq!(github.get("3").unwrap().status, Status::Deleted);
        // Repeating is just as explicit, and Restore still works afterwards.
        assert!(matches!(
            github.purge_trashed("3").unwrap(),
            ProviderPurgeOutcome::Retained { .. }
        ));
        assert_eq!(github.restore("3", now()).unwrap().status, Status::Verified);
    }
}

#[test]
fn gh_5_other_graphql_errors_fail_instead_of_claiming_success() {
    struct Broken;
    impl GitHubTransport for Broken {
        fn request(
            &self,
            method: &str,
            _: &str,
            _: &[(&str, String)],
            _: Option<&Value>,
        ) -> Result<HttpResponse, String> {
            Ok(if method == "GET" {
                FakeGitHub::reply(
                    200,
                    json!({"number": 4, "node_id": "I_4", "title": "t", "body": "",
                           "state": "closed", "state_reason": "not_planned",
                           "html_url": "https://github.com/acme/widgets/issues/4",
                           "created_at": "2026-11-01T00:00:00Z", "updated_at": "2026-11-01T00:00:00Z",
                           "closed_at": "2026-11-01T00:00:00Z",
                           "labels": [{"name": "status:deleted"}], "assignees": []}),
                )
            } else {
                FakeGitHub::reply(
                    200,
                    json!({"data": null, "errors": [{"type": "INTERNAL", "message": "Something went wrong"}]}),
                )
            })
        }
    }
    let mut config = GitHubConfig::new("github-main", "acme/widgets", "test-token");
    config.api_base = "https://api.test".into();
    let github = GitHubProvider::new(config, Arc::new(Broken));
    let error = github.purge_trashed("4").unwrap_err();
    assert!(
        error.to_string().contains("Something went wrong"),
        "{error}"
    );
}

#[test]
fn gh_5_expiry_and_empty_trash_purge_through_the_shared_provider_path() {
    let fake = FakeGitHub::new(DeleteAccess::Admin);
    for number in [1, 2, 3] {
        fake.seed(number, "closed", Some("completed"), &["category:bug"]);
    }
    let github = provider(&fake);
    for number in ["1", "2", "3"] {
        delete(&github, number);
    }
    fake.set_updated_at(1, "2026-10-01T00:00:00Z");
    fake.set_updated_at(2, "not a time");

    // Expiry: only the ticket past retention goes; an unparseable time is kept.
    let expired = purge_provider_trash(&github, &now(), Some(30)).unwrap();
    assert_eq!(
        expired,
        ProviderTrashPurge {
            purged: vec!["acme/widgets#1".into()],
            retained: vec![],
        }
    );
    assert!(!fake.exists(1) && fake.exists(2) && fake.exists(3));

    // Empty Trash removes the rest; repeating it finds nothing.
    let emptied = purge_provider_trash(&github, &now(), None).unwrap();
    assert_eq!(emptied.purged.len(), 2, "{emptied:?}");
    assert!(emptied.retained.is_empty());
    assert_eq!(
        purge_provider_trash(&github, &now(), None).unwrap(),
        ProviderTrashPurge::default()
    );
}

#[test]
fn gh_5_empty_trash_without_permission_reports_every_retained_ticket() {
    let fake = FakeGitHub::new(DeleteAccess::UserWithoutAdmin);
    fake.seed(
        1,
        "closed",
        Some("completed"),
        &["category:bug", "status:verified"],
    );
    fake.seed(2, "open", None, &["category:bug"]);
    let github = provider(&fake);
    delete(&github, "1");
    delete(&github, "2");
    let report = purge_provider_trash(&github, &now(), None).unwrap();
    assert!(report.purged.is_empty());
    assert_eq!(
        report
            .retained
            .iter()
            .map(|ticket| ticket.qualified_id.as_str())
            .collect::<Vec<_>>(),
        ["github-main:1", "github-main:2"]
    );
    assert!(
        report
            .retained
            .iter()
            .all(|ticket| ticket.message.starts_with(PURGE_NOT_PERMITTED))
    );
    assert!(fake.exists(1) && fake.exists(2));
    // Granting permission later lets the next run finish the job.
    *fake.access.lock().unwrap() = DeleteAccess::Admin;
    let report = purge_provider_trash(&github, &now(), None).unwrap();
    assert_eq!(report.purged.len(), 2);
    assert!(!fake.exists(1) && !fake.exists(2));
}

#[test]
fn hs2_dag8p8_moving_out_of_github_closes_the_source_with_exactly_one_destination() {
    // Parity with the Jira move fix: GitHub can close, so a move closes its source as
    // not planned, and repeating the operation id neither duplicates nor reopens.
    let fake = FakeGitHub::new(DeleteAccess::Admin);
    fake.seed(3, "open", None, &[]);
    let dir = tempfile::tempdir().unwrap();
    let store = hotsheet_ticketing::FsStore::init(
        dir.path(),
        &hotsheet_ticketing::StoreMetadata::new("DST"),
    )
    .unwrap();
    let registry = hotsheet_ticketing::ProviderRegistry::default();
    registry.register(Arc::new(provider(&fake))).unwrap();
    registry
        .register(Arc::new(hotsheet_ticketing::GitProvider::new(
            "local",
            store.clone(),
        )))
        .unwrap();
    let source = hotsheet_ticketing::TicketRef {
        connection_id: "github-main".into(),
        native_id: "3".into(),
    };
    let mut destinations = Vec::new();
    for _ in 0..2 {
        let moved =
            hotsheet_ticketing::move_between(&registry, source.clone(), "local", "move-3", now())
                .unwrap();
        assert!(moved.moved);
        destinations.push(moved.destination.qualified());
        assert_eq!(store.list_tickets().unwrap().len(), 1);
        assert_eq!(fake.field(3, "state"), "closed");
        assert_eq!(fake.field(3, "state_reason"), "not_planned");
    }
    assert_eq!(destinations[0], destinations[1]);
}
