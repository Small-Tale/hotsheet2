//! HS2-DBKM89: Delete moves a GitLab issue to Trash, Restore brings back its exact prior
//! state, and expiry or Empty Trash deletes it only when GitLab allows it (GH-5 parity).
//! Synthetic issues only.

use super::super::{GitLabConfig, GitLabProvider, HttpResponse, HttpTransport};
use super::*;
use hotsheet_model::{CloseReason, Timestamp};
use hotsheet_ticketing::{
    ProviderPatch, ProviderTrashPurge, TicketProvider, TicketQuery, purge_provider_trash,
};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

/// How the fake answers `DELETE /projects/:id/issues/:iid`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DeleteAccess {
    /// A project Owner (or instance admin): 204 No Content, the issue is gone.
    Owner,
    /// A Maintainer or Developer: `403 {"message":"403 Forbidden"}`, as GitLab answers.
    Maintainer,
    /// A token without the `api` scope: GitLab's OAuth-style `insufficient_scope` 403.
    ReadScope,
    /// The rate limiter: 403 with `Retry-After`, which is not a permission refusal.
    RateLimited,
    /// An unexpected server failure.
    ServerError,
}

/// A stateful GitLab Issues fake following the REST v4 response shapes: `PUT` replaces the
/// comma-separated `labels` and applies `state_event`, moving `state` between `opened` and
/// `closed` and `closed_at`; `DELETE` answers 204 for an Owner and 403 otherwise; a deleted
/// issue answers `404 Issue Not Found` (or 410 when `gone` is set) and lists leave it out.
struct FakeGitLab {
    issues: Mutex<BTreeMap<u64, Value>>,
    deleted: Mutex<Vec<u64>>,
    access: Mutex<DeleteAccess>,
    gone: Mutex<bool>,
    clock: Mutex<u64>,
    requests: Mutex<Vec<(String, String, Option<Value>)>>,
}

impl FakeGitLab {
    fn new(access: DeleteAccess) -> Arc<Self> {
        Arc::new(Self {
            issues: Mutex::default(),
            deleted: Mutex::default(),
            access: Mutex::new(access),
            gone: Mutex::new(false),
            clock: Mutex::new(0),
            requests: Mutex::default(),
        })
    }

    fn tick(&self) -> String {
        let mut clock = self.clock.lock().unwrap();
        *clock += 1;
        format!("2026-11-20T00:{:02}:{:02}.000Z", *clock / 60, *clock % 60)
    }

    fn seed(&self, iid: u64, state: &str, labels: &[&str]) {
        let closed = state == "closed";
        self.issues.lock().unwrap().insert(
            iid,
            json!({
                "id": 9000 + iid,
                "iid": iid,
                "project_id": 42,
                "title": format!("Synthetic issue {iid}"),
                "description": "Synthetic details.",
                "state": state,
                "web_url": format!("https://gitlab.test/acme/repo/-/issues/{iid}"),
                "created_at": "2026-11-01T00:00:00.000Z",
                "updated_at": "2026-11-01T00:01:00.000Z",
                "closed_at": closed.then_some("2026-11-01T00:01:00.000Z"),
                "closed_by": null,
                "labels": labels,
                "assignees": [],
                "author": {"id": 1, "username": "synthetic"},
                "type": "ISSUE",
                "issue_type": "issue"
            }),
        );
    }

    fn set_updated_at(&self, iid: u64, at: &str) {
        self.issues.lock().unwrap().get_mut(&iid).unwrap()["updated_at"] = json!(at);
    }

    fn labels(&self, iid: u64) -> Vec<String> {
        let mut names = self.issues.lock().unwrap()[&iid]["labels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|label| label.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn field(&self, iid: u64, name: &str) -> Value {
        self.issues.lock().unwrap()[&iid][name].clone()
    }

    fn exists(&self, iid: u64) -> bool {
        self.issues.lock().unwrap().contains_key(&iid)
    }

    fn count(&self, method: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(seen, _, _)| seen == method)
            .count()
    }

    fn reply(status: u16, body: Value) -> HttpResponse {
        HttpResponse {
            status,
            headers: HashMap::from([("x-next-page".to_string(), String::new())]),
            body: body.to_string(),
        }
    }

    fn put(&self, iid: u64, body: &Value) -> HttpResponse {
        let now = self.tick();
        let mut issues = self.issues.lock().unwrap();
        let Some(issue) = issues.get_mut(&iid) else {
            return Self::reply(404, json!({"message": "404 Issue Not Found"}));
        };
        if let Some(labels) = body.get("labels").and_then(Value::as_str) {
            issue["labels"] = json!(
                labels
                    .split(',')
                    .filter(|name| !name.is_empty())
                    .collect::<Vec<_>>()
            );
        }
        for field in ["title", "description"] {
            if let Some(value) = body.get(field) {
                issue[field] = value.clone();
            }
        }
        match body.get("state_event").and_then(Value::as_str) {
            Some("close") if issue["state"] != "closed" => {
                issue["state"] = json!("closed");
                issue["closed_at"] = json!(now);
            }
            Some("reopen") if issue["state"] == "closed" => {
                issue["state"] = json!("opened");
                issue["closed_at"] = Value::Null;
            }
            _ => {}
        }
        issue["updated_at"] = json!(now);
        Self::reply(200, issue.clone())
    }

    fn delete(&self, iid: u64) -> HttpResponse {
        match *self.access.lock().unwrap() {
            DeleteAccess::Maintainer => {
                return Self::reply(403, json!({"message": "403 Forbidden"}));
            }
            DeleteAccess::ReadScope => {
                return Self::reply(
                    403,
                    json!({
                        "error": "insufficient_scope",
                        "error_description": "The request requires higher privileges than provided by the access token.",
                        "scope": "api"
                    }),
                );
            }
            DeleteAccess::RateLimited => {
                let mut response = Self::reply(403, json!({"message": "403 Forbidden"}));
                response.headers.insert("retry-after".into(), "30".into());
                return response;
            }
            DeleteAccess::ServerError => {
                return Self::reply(500, json!({"message": "500 Internal Server Error"}));
            }
            DeleteAccess::Owner => {}
        }
        if self.issues.lock().unwrap().remove(&iid).is_none() {
            return Self::reply(404, json!({"message": "404 Issue Not Found"}));
        }
        self.deleted.lock().unwrap().push(iid);
        HttpResponse {
            status: 204,
            headers: HashMap::new(),
            body: String::new(),
        }
    }
}

impl HttpTransport for FakeGitLab {
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
        let rest = path
            .strip_prefix("https://gitlab.test/api/v4/projects/acme%2Frepo/issues")
            .unwrap_or("-");
        if rest.is_empty() && method == "GET" {
            let issues = self.issues.lock().unwrap();
            return Ok(Self::reply(200, json!(issues.values().collect::<Vec<_>>())));
        }
        if method == "GET" && (rest.ends_with("/notes") || rest.ends_with("/resource_state_events"))
        {
            return Ok(Self::reply(200, json!([])));
        }
        let Ok(iid) = rest.trim_start_matches('/').parse::<u64>() else {
            return Ok(Self::reply(404, json!({"message": "404 Not found"})));
        };
        if self.deleted.lock().unwrap().contains(&iid) && *self.gone.lock().unwrap() {
            return Ok(Self::reply(410, json!({"message": "410 Gone"})));
        }
        Ok(match method {
            "GET" => match self.issues.lock().unwrap().get(&iid) {
                Some(issue) => Self::reply(200, issue.clone()),
                None => Self::reply(404, json!({"message": "404 Issue Not Found"})),
            },
            "PUT" => self.put(iid, body.unwrap_or(&Value::Null)),
            "DELETE" => self.delete(iid),
            _ => Self::reply(405, json!({"message": "405 Method Not Allowed"})),
        })
    }
}

fn provider(fake: &Arc<FakeGitLab>) -> GitLabProvider {
    GitLabProvider::new(
        GitLabConfig {
            connection_id: "gitlab-main".into(),
            project: "acme/repo".into(),
            api_base: "https://gitlab.test/api/v4".into(),
            token: "synthetic-token".into(),
            default: false,
        },
        fake.clone(),
    )
}

fn now() -> Timestamp {
    Timestamp::new("2026-11-25T00:00:00Z")
}

fn delete(gitlab: &GitLabProvider, iid: &str) -> ApiTicket {
    gitlab
        .update(
            iid,
            now(),
            ProviderPatch {
                status: Some(Status::Deleted),
                ..Default::default()
            },
        )
        .unwrap()
}

/// Each starting Hot Sheet status GitLab represents, its `state`, the labels Hot Sheet
/// wrote, and the close reason the ticket reads back with.
fn starting_states() -> Vec<(Status, &'static str, Vec<&'static str>, Option<CloseReason>)> {
    vec![
        (
            Status::NotStarted,
            "opened",
            vec!["category:bug", "priority:high", "customer"],
            None,
        ),
        (
            Status::Started,
            "opened",
            vec!["category:bug", "priority:high", "status:started"],
            None,
        ),
        (
            Status::Backlog,
            "opened",
            vec!["category:bug", "priority:default", "status:backlog"],
            None,
        ),
        (
            Status::Completed,
            "closed",
            vec!["category:bug", "priority:high", "customer"],
            Some(CloseReason::Completed),
        ),
    ]
}

#[test]
fn hs2_dbkm89_delete_trashes_every_status_and_restore_returns_it_exactly() {
    for (status, state, labels, close_reason) in starting_states() {
        let fake = FakeGitLab::new(DeleteAccess::Owner);
        fake.seed(5, state, &labels);
        let gitlab = provider(&fake);
        let before = gitlab.get("5").unwrap();
        assert_eq!(before.status, status, "{labels:?}");
        let closed_at = fake.field(5, "closed_at");

        // Delete: closed, in Trash, never Completed (the reported bug).
        let trashed = delete(&gitlab, "5");
        assert_eq!(trashed.status, Status::Deleted, "{status:?}");
        assert_eq!(trashed.close_reason, close_reason, "{status:?}");
        assert_eq!(trashed.tags, before.tags, "ordinary tags survive Delete");
        assert_eq!(fake.field(5, "state"), "closed");
        let stored = fake.labels(5);
        assert!(stored.contains(&"status:deleted".to_string()), "{stored:?}");
        assert!(
            stored.contains(&format!("deleted-from:{}", status_name(status).unwrap())),
            "{stored:?}"
        );
        assert!(
            !stored.iter().any(|label| label == "status:started"
                || label == "status:backlog"
                || label.starts_with("closed:")),
            "{stored:?}"
        );
        assert_eq!(gitlab.get("5").unwrap().status, Status::Deleted);

        // A repeated Delete is idempotent: the same labels, the same restore target.
        let again = delete(&gitlab, "5");
        assert_eq!(again.status, Status::Deleted);
        assert_eq!(fake.labels(5), stored, "{status:?}");

        // The board's Trash query finds it; an active status query does not.
        let trash = gitlab
            .query(&TicketQuery {
                status: Some(Status::Deleted),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(trash.len(), 1);
        let completed = gitlab
            .query(&TicketQuery {
                status: Some(Status::Completed),
                ..Default::default()
            })
            .unwrap();
        assert!(completed.is_empty(), "a trashed issue is never Completed");

        // Restore brings back the status, GitLab state, reason, and tags; no Trash label.
        let restored = gitlab.restore("5", now()).unwrap();
        assert_eq!(restored.status, status, "{labels:?}");
        assert_eq!(restored.close_reason, close_reason, "{labels:?}");
        assert_eq!(restored.tags, before.tags);
        assert_eq!(fake.field(5, "state"), state);
        let mut expected = labels
            .iter()
            .map(|label| label.to_string())
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(fake.labels(5), expected, "{status:?}");
        if state == "opened" {
            assert_eq!(fake.field(5, "closed_at"), Value::Null);
        } else {
            assert_eq!(
                fake.field(5, "closed_at"),
                closed_at,
                "a Completed ticket keeps its original close time through Delete and Restore"
            );
        }

        // Restoring again is a NotInTrash refusal with no write.
        let puts = fake.count("PUT");
        assert!(matches!(
            gitlab.restore("5", now()),
            Err(ProviderError::Operation(OpError::NotInTrash(found))) if found == status
        ));
        assert_eq!(fake.count("PUT"), puts, "no write");
    }
}

#[test]
fn hs2_dbkm89_restore_sends_only_the_state_event_the_prior_status_needs() {
    let fake = FakeGitLab::new(DeleteAccess::Owner);
    fake.seed(1, "closed", &["category:bug"]);
    fake.seed(2, "opened", &["category:bug", "status:started"]);
    let gitlab = provider(&fake);
    delete(&gitlab, "1");
    delete(&gitlab, "2");
    gitlab.restore("1", now()).unwrap();
    gitlab.restore("2", now()).unwrap();
    let puts = fake
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|(method, _, _)| method == "PUT")
        .map(|(_, url, body)| (url.rsplit('/').next().unwrap().to_string(), body.clone()))
        .collect::<Vec<_>>();
    let restore_1 = &puts[2].1.as_ref().unwrap();
    assert_eq!(puts[2].0, "1");
    assert!(restore_1.get("state_event").is_none(), "{restore_1}");
    assert_eq!(restore_1["labels"], "category:bug,priority:default");
    let restore_2 = &puts[3].1.as_ref().unwrap();
    assert_eq!(puts[3].0, "2");
    assert_eq!(restore_2["state_event"], "reopen");
    assert_eq!(
        restore_2["labels"],
        "category:bug,priority:default,status:started"
    );
}

#[test]
fn hs2_dbkm89_a_trashed_issue_without_a_restore_label_reopens_as_not_started() {
    let fake = FakeGitLab::new(DeleteAccess::Owner);
    // Trashed outside Hot Sheet: someone added the label by hand.
    fake.seed(7, "closed", &["category:task", "status:deleted"]);
    let gitlab = provider(&fake);
    let ticket = gitlab.get("7").unwrap();
    assert_eq!(ticket.status, Status::Deleted);
    assert_eq!(ticket.close_reason, None);
    let restored = gitlab.restore("7", now()).unwrap();
    assert_eq!(restored.status, Status::NotStarted);
    assert_eq!(fake.field(7, "state"), "opened");
}

#[test]
fn hs2_dbkm89_reopening_a_trashed_issue_on_gitlab_takes_it_out_of_trash() {
    let fake = FakeGitLab::new(DeleteAccess::Owner);
    fake.seed(
        8,
        "opened",
        &["category:task", "status:deleted", "deleted-from:completed"],
    );
    let ticket = provider(&fake).get("8").unwrap();
    assert_eq!(
        ticket.status,
        Status::NotStarted,
        "a stale closed-only label is ignored"
    );
    assert_eq!(ticket.close_reason, None);
    assert!(ticket.tags.is_empty(), "Trash labels are never tags");
}

#[test]
fn hs2_dbkm89_moving_a_trashed_ticket_to_another_status_drops_the_trash_labels() {
    let fake = FakeGitLab::new(DeleteAccess::Owner);
    fake.seed(9, "closed", &["category:bug", "customer"]);
    let gitlab = provider(&fake);
    delete(&gitlab, "9");
    let started = gitlab
        .update(
            "9",
            now(),
            ProviderPatch {
                status: Some(Status::Started),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(started.status, Status::Started);
    assert_eq!(fake.field(9, "state"), "opened");
    assert_eq!(
        fake.labels(9),
        [
            "category:bug",
            "customer",
            "priority:default",
            "status:started"
        ]
    );
}

#[test]
fn hs2_dbkm89_purge_deletes_only_trashed_issues_and_is_idempotent() {
    let fake = FakeGitLab::new(DeleteAccess::Owner);
    fake.seed(1, "closed", &["category:bug"]);
    fake.seed(2, "opened", &["category:bug"]);
    let gitlab = provider(&fake);
    delete(&gitlab, "1");

    // An issue outside Trash is refused before any DELETE.
    assert!(matches!(
        gitlab.purge_trashed("2"),
        Err(ProviderError::Operation(OpError::NotInTrash(
            Status::NotStarted
        )))
    ));
    assert_eq!(fake.count("DELETE"), 0);

    assert_eq!(
        gitlab.purge_trashed("1").unwrap(),
        ProviderPurgeOutcome::Purged
    );
    assert!(!fake.exists(1));
    let request = fake
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|(method, _, _)| method == "DELETE")
        .cloned()
        .unwrap();
    assert_eq!(
        request.1,
        "https://gitlab.test/api/v4/projects/acme%2Frepo/issues/1"
    );
    assert!(request.2.is_none());

    // Repeats: the deleted issue answers 404, which still counts as purged.
    assert_eq!(
        gitlab.purge_trashed("1").unwrap(),
        ProviderPurgeOutcome::Purged
    );
    assert_eq!(fake.count("DELETE"), 1, "no second delete");
    assert!(matches!(
        gitlab.get("1"),
        Err(ProviderError::NotFound { .. })
    ));

    // A server that answers 410 Gone for the deleted issue counts it as purged too.
    *fake.gone.lock().unwrap() = true;
    assert_eq!(
        gitlab.purge_trashed("1").unwrap(),
        ProviderPurgeOutcome::Purged
    );
    assert!(matches!(
        gitlab.get("1"),
        Err(ProviderError::NotFound { .. })
    ));
}

#[test]
fn hs2_dbkm89_a_delete_racing_another_purge_counts_404_and_410_as_purged() {
    // The issue is in Trash when read, but GitLab no longer has it when DELETE arrives.
    for status in [404, 410] {
        struct Racing(u16);
        impl HttpTransport for Racing {
            fn request(
                &self,
                method: &str,
                _: &str,
                _: &[(&str, String)],
                _: Option<&Value>,
            ) -> Result<HttpResponse, String> {
                Ok(if method == "GET" {
                    FakeGitLab::reply(
                        200,
                        json!({"iid": 4, "title": "t", "description": "", "state": "closed",
                               "web_url": "https://gitlab.test/acme/repo/-/issues/4",
                               "created_at": "2026-11-01T00:00:00Z", "updated_at": "2026-11-01T00:00:00Z",
                               "closed_at": "2026-11-01T00:00:00Z",
                               "labels": ["status:deleted"], "assignees": []}),
                    )
                } else {
                    FakeGitLab::reply(self.0, json!({"message": format!("{} Gone", self.0)}))
                })
            }
        }
        let gitlab = GitLabProvider::new(
            GitLabConfig {
                connection_id: "gitlab-main".into(),
                project: "acme/repo".into(),
                api_base: "https://gitlab.test/api/v4".into(),
                token: "synthetic-token".into(),
                default: false,
            },
            Arc::new(Racing(status)),
        );
        assert_eq!(
            gitlab.purge_trashed("4").unwrap(),
            ProviderPurgeOutcome::Purged,
            "{status}"
        );
    }
}

#[test]
fn hs2_dbkm89_purge_without_permission_keeps_the_issue_trashed_and_says_why() {
    for (access, said) in [
        (DeleteAccess::Maintainer, "403 Forbidden"),
        (DeleteAccess::ReadScope, "requires higher privileges"),
    ] {
        let fake = FakeGitLab::new(access);
        fake.seed(3, "closed", &["category:bug"]);
        let gitlab = provider(&fake);
        delete(&gitlab, "3");
        let outcome = gitlab.purge_trashed("3").unwrap();
        let ProviderPurgeOutcome::Retained { message } = outcome else {
            panic!("a refused delete must not read as purged");
        };
        assert!(message.starts_with(PURGE_NOT_PERMITTED), "{message}");
        assert!(message.contains(said), "{access:?}: {message}");
        assert!(fake.exists(3));
        assert_eq!(gitlab.get("3").unwrap().status, Status::Deleted);
        // Repeating is just as explicit, and Restore still works afterwards.
        assert!(matches!(
            gitlab.purge_trashed("3").unwrap(),
            ProviderPurgeOutcome::Retained { .. }
        ));
        assert_eq!(
            gitlab.restore("3", now()).unwrap().status,
            Status::Completed
        );
    }
}

#[test]
fn hs2_dbkm89_rate_limits_and_server_errors_fail_instead_of_claiming_an_outcome() {
    let fake = FakeGitLab::new(DeleteAccess::RateLimited);
    fake.seed(4, "closed", &["category:bug"]);
    let gitlab = provider(&fake);
    delete(&gitlab, "4");
    assert!(matches!(
        gitlab.purge_trashed("4"),
        Err(ProviderError::RateLimited {
            retry_after_seconds: Some(30),
            ..
        })
    ));
    *fake.access.lock().unwrap() = DeleteAccess::ServerError;
    let error = gitlab.purge_trashed("4").unwrap_err();
    assert!(error.to_string().contains("500"), "{error}");
    assert!(fake.exists(4));
}

#[test]
fn hs2_dbkm89_expiry_and_empty_trash_purge_through_the_shared_provider_path() {
    let fake = FakeGitLab::new(DeleteAccess::Owner);
    for iid in [1, 2, 3] {
        fake.seed(iid, "closed", &["category:bug"]);
    }
    let gitlab = provider(&fake);
    for iid in ["1", "2", "3"] {
        delete(&gitlab, iid);
    }
    fake.set_updated_at(1, "2026-10-01T00:00:00.000Z");
    fake.set_updated_at(2, "not a time");

    // Expiry: only the ticket past retention goes; an unparseable time is kept.
    let expired = purge_provider_trash(&gitlab, &now(), Some(30)).unwrap();
    assert_eq!(
        expired,
        ProviderTrashPurge {
            purged: vec!["acme/repo#1".into()],
            retained: vec![],
        }
    );
    assert!(!fake.exists(1) && fake.exists(2) && fake.exists(3));

    // Empty Trash removes the rest; repeating it finds nothing.
    let emptied = purge_provider_trash(&gitlab, &now(), None).unwrap();
    assert_eq!(emptied.purged.len(), 2, "{emptied:?}");
    assert!(emptied.retained.is_empty());
    assert_eq!(
        purge_provider_trash(&gitlab, &now(), None).unwrap(),
        ProviderTrashPurge::default()
    );
}

#[test]
fn hs2_dbkm89_empty_trash_without_permission_reports_every_retained_ticket() {
    let fake = FakeGitLab::new(DeleteAccess::Maintainer);
    fake.seed(1, "closed", &["category:bug"]);
    fake.seed(2, "opened", &["category:bug"]);
    let gitlab = provider(&fake);
    delete(&gitlab, "1");
    delete(&gitlab, "2");
    let report = purge_provider_trash(&gitlab, &now(), None).unwrap();
    assert!(report.purged.is_empty());
    assert_eq!(
        report
            .retained
            .iter()
            .map(|ticket| ticket.qualified_id.as_str())
            .collect::<Vec<_>>(),
        ["gitlab-main:1", "gitlab-main:2"]
    );
    assert!(
        report
            .retained
            .iter()
            .all(|ticket| ticket.message.starts_with(PURGE_NOT_PERMITTED))
    );
    assert!(fake.exists(1) && fake.exists(2));
    // Becoming an Owner later lets the next run finish the job.
    *fake.access.lock().unwrap() = DeleteAccess::Owner;
    let report = purge_provider_trash(&gitlab, &now(), None).unwrap();
    assert_eq!(report.purged.len(), 2);
    assert!(!fake.exists(1) && !fake.exists(2));
}

/// HS2-DBKM89: Delete, Restore, and permanent deletion against the live GitLab test
/// project. The token decides the last step: an Owner removes the issue; anyone else must
/// see it retained in Trash, and the test then restores and closes it so nothing is left in
/// Trash.
#[test]
#[ignore = "writes to the live GitLab test project; set HOTSHEET_GITLAB_LIVE_PROJECT, HOTSHEET_GITLAB_LIVE_TOKEN and HOTSHEET_GITLAB_LIVE_WRITE=1"]
fn gitlab_live_trash_delete_restore_and_permission_checked_purge() {
    assert_eq!(
        std::env::var("HOTSHEET_GITLAB_LIVE_WRITE").as_deref(),
        Ok("1"),
        "write access to the live project must be opted into"
    );
    let connection = hotsheet_ticketing::ProviderConnection {
        id: "gitlab-live".into(),
        provider: "gitlab".into(),
        locator: std::env::var("HOTSHEET_GITLAB_LIVE_PROJECT").expect("live project"),
        name: None,
        default: false,
        settings: json!({"api_base": std::env::var("HOTSHEET_GITLAB_LIVE_API_BASE")
            .unwrap_or_else(|_| "https://gitlab.com/api/v4".into())}),
        disabled: false,
    };
    let gitlab = GitLabProvider::live(
        GitLabConfig::from_connection(
            &connection,
            std::env::var("HOTSHEET_GITLAB_LIVE_TOKEN").expect("live token"),
        )
        .unwrap(),
    );
    let created = gitlab
        .create(
            hotsheet_ticketing::MutationContext {
                now: now(),
                generated_id: hotsheet_model::Ulid::new(),
            },
            hotsheet_ticketing::ProviderDraft {
                group: None,
                title: "Hot Sheet live trash check (HS2-DBKM89)".into(),
                category: "test".into(),
                priority: hotsheet_model::Priority::Default,
                status: Status::NotStarted,
                details: "Created by an opt-in Hot Sheet provider validation.".into(),
                tags: vec![],
                up_next: false,
                blocked_by: vec![],
                transfer: None,
                created_by: None,
            },
        )
        .unwrap();
    let id = created.native_id;
    let status = |status| ProviderPatch {
        status: Some(status),
        ..Default::default()
    };
    gitlab
        .update(&id, now(), status(Status::Completed))
        .unwrap();
    let trashed = gitlab.update(&id, now(), status(Status::Deleted)).unwrap();
    assert_eq!(
        trashed.status,
        Status::Deleted,
        "the reported bug read Completed"
    );
    assert_eq!(gitlab.get(&id).unwrap().status, Status::Deleted);
    let restored = gitlab.restore(&id, now()).unwrap();
    assert_eq!(restored.status, Status::Completed);
    assert_eq!(restored.close_reason, Some(CloseReason::Completed));
    gitlab.update(&id, now(), status(Status::Deleted)).unwrap();
    match gitlab.purge_trashed(&id).unwrap() {
        ProviderPurgeOutcome::Purged => {
            assert!(matches!(
                gitlab.get(&id),
                Err(ProviderError::NotFound { .. })
            ));
        }
        ProviderPurgeOutcome::Retained { message } => {
            assert!(message.starts_with(PURGE_NOT_PERMITTED), "{message}");
            assert_eq!(gitlab.get(&id).unwrap().status, Status::Deleted);
            assert_eq!(
                gitlab.restore(&id, now()).unwrap().status,
                Status::Completed
            );
        }
    }
}

#[test]
fn hs2_dag8p8_moving_out_of_gitlab_closes_the_source_with_exactly_one_destination() {
    // Parity with the Jira move fix: GitLab can close, so a move closes its source, and
    // repeating the operation id neither duplicates the destination nor reopens it.
    let fake = FakeGitLab::new(DeleteAccess::Owner);
    fake.seed(5, "opened", &[]);
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
        connection_id: "gitlab-main".into(),
        native_id: "5".into(),
    };
    let mut destinations = Vec::new();
    for _ in 0..2 {
        let moved =
            hotsheet_ticketing::move_between(&registry, source.clone(), "local", "move-5", now())
                .unwrap();
        assert!(moved.moved);
        destinations.push(moved.destination.qualified());
        assert_eq!(store.list_tickets().unwrap().len(), 1);
        assert_eq!(fake.field(5, "state"), "closed");
    }
    assert_eq!(destinations[0], destinations[1]);
}
