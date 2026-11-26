//! HS2-73Z1R8: Delete moves a Jira issue to a label-only Trash without touching its workflow
//! status, Restore removes the label so the ticket returns to exactly the status it had, and
//! expiry or Empty Trash deletes the issue only when Jira allows it (GH-5 parity). Synthetic
//! issues only.

use super::super::{HttpResponse, HttpTransport, JiraConfig, JiraProvider, text_to_adf};
use super::*;
use hotsheet_model::{CloseReason, Status, Timestamp};
use hotsheet_ticketing::{
    ProviderPatch, ProviderTrashPurge, TicketCollection, TicketQuery, purge_provider_trash,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

/// How the fake answers `DELETE /rest/api/3/issue/{key}`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DeleteAccess {
    /// The Delete Issues permission: 204 No Content, the issue is gone.
    Allowed,
    /// No Delete Issues permission: Jira's 403 `errorMessages` body.
    NoPermission,
    /// A parent issue with subtasks and `deleteSubtasks` unset: Jira's 400.
    HasSubtasks,
    /// Jira's rate limiter: 429 with `Retry-After`.
    RateLimited,
    /// An unexpected server failure.
    ServerError,
}

/// A stateful Jira Cloud fake following REST v3 shapes: `GET issue/{key}` returns the issue
/// with `fields.status.statusCategory`; `PUT issue/{key}` applies `fields` (labels replace)
/// and the `update.labels` add/remove operations and answers 204; `POST search/jql` honours
/// the `labels = "..."` clauses and answers `{issues, isLast}`; a missing issue answers 404
/// with Jira's `errorMessages`.
struct FakeJira {
    issues: Mutex<BTreeMap<String, Value>>,
    access: Mutex<DeleteAccess>,
    clock: Mutex<u64>,
    requests: Mutex<Vec<(String, String, Option<Value>)>>,
    /// How many upcoming `PUT issue/{key}` requests fail with a 503 before applying.
    failing_puts: Mutex<usize>,
}

const MISSING: &str = "Issue does not exist or you do not have permission to see it.";

impl FakeJira {
    fn new(access: DeleteAccess) -> Arc<Self> {
        Arc::new(Self {
            issues: Mutex::default(),
            access: Mutex::new(access),
            clock: Mutex::new(0),
            requests: Mutex::default(),
            failing_puts: Mutex::new(0),
        })
    }

    fn tick(&self) -> String {
        let mut clock = self.clock.lock().unwrap();
        *clock += 1;
        format!(
            "2026-11-20T00:{:02}:{:02}.000+0000",
            *clock / 60,
            *clock % 60
        )
    }

    /// Seed an issue in a status of the given category (`new`, `indeterminate`, `done`).
    fn seed(&self, key: &str, category: &str, labels: &[&str]) {
        let (name, id) = match category {
            "done" => ("Done", 3),
            "indeterminate" => ("In Progress", 4),
            _ => ("To Do", 2),
        };
        let done = category == "done";
        self.issues.lock().unwrap().insert(
            key.into(),
            json!({
                "id": "10001",
                "key": key,
                "self": format!("https://jira.test/rest/api/3/issue/{key}"),
                "fields": {
                    "summary": format!("Synthetic issue {key}"),
                    "description": text_to_adf("Synthetic details."),
                    "status": {
                        "name": name,
                        "id": "10000",
                        "statusCategory": {"id": id, "key": category, "name": name}
                    },
                    "priority": {"name": "Medium", "id": "3"},
                    "issuetype": {"name": "Task", "id": "10002"},
                    "labels": labels,
                    "assignee": null,
                    "created": "2026-11-01T00:00:00.000+0000",
                    "updated": "2026-11-01T00:01:00.000+0000",
                    "resolutiondate": done.then_some("2026-11-01T00:01:00.000+0000"),
                    "attachment": []
                }
            }),
        );
    }

    fn set_updated(&self, key: &str, at: &str) {
        self.issues.lock().unwrap().get_mut(key).unwrap()["fields"]["updated"] = json!(at);
    }

    fn labels(&self, key: &str) -> Vec<String> {
        let mut names = self.issues.lock().unwrap()[key]["fields"]["labels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|label| label.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn category(&self, key: &str) -> String {
        self.issues.lock().unwrap()[key]["fields"]["status"]["statusCategory"]["key"]
            .as_str()
            .unwrap()
            .into()
    }

    fn exists(&self, key: &str) -> bool {
        self.issues.lock().unwrap().contains_key(key)
    }

    fn count(&self, method: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(seen, _, _)| seen == method)
            .count()
    }

    fn last(&self, method: &str) -> (String, Option<Value>) {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|(seen, _, _)| seen == method)
            .map(|(_, url, body)| (url.clone(), body.clone()))
            .unwrap()
    }

    fn reply(status: u16, body: Value) -> HttpResponse {
        HttpResponse {
            status,
            headers: HashMap::new(),
            body: body.to_string(),
        }
    }

    fn no_content() -> HttpResponse {
        HttpResponse {
            status: 204,
            headers: HashMap::new(),
            body: String::new(),
        }
    }

    fn missing() -> HttpResponse {
        Self::reply(404, json!({"errorMessages": [MISSING], "errors": {}}))
    }

    fn put(&self, key: &str, body: &Value) -> HttpResponse {
        {
            let mut failing = self.failing_puts.lock().unwrap();
            if *failing > 0 {
                *failing -= 1;
                return Self::reply(
                    503,
                    json!({"errorMessages": ["Service temporarily unavailable"], "errors": {}}),
                );
            }
        }
        let now = self.tick();
        let mut issues = self.issues.lock().unwrap();
        let Some(issue) = issues.get_mut(key) else {
            return Self::missing();
        };
        if let Some(fields) = body.get("fields").and_then(Value::as_object) {
            for (name, value) in fields {
                issue["fields"][name] = value.clone();
            }
        }
        for operation in body
            .pointer("/update/labels")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let labels = issue["fields"]["labels"].as_array_mut().unwrap();
            if let Some(add) = operation.get("add") {
                if !labels.contains(add) {
                    labels.push(add.clone());
                }
            } else if let Some(remove) = operation.get("remove") {
                labels.retain(|label| label != remove);
            }
        }
        issue["fields"]["updated"] = json!(now);
        Self::no_content()
    }

    fn delete(&self, key: &str) -> HttpResponse {
        if !self.exists(key) {
            return Self::missing();
        }
        match *self.access.lock().unwrap() {
            DeleteAccess::Allowed => {
                self.issues.lock().unwrap().remove(key);
                Self::no_content()
            }
            DeleteAccess::NoPermission => Self::reply(
                403,
                json!({
                    "errorMessages": ["You do not have permission to delete issues in this project."],
                    "errors": {}
                }),
            ),
            DeleteAccess::HasSubtasks => Self::reply(
                400,
                json!({
                    "errorMessages": ["The issue has subtasks. Set deleteSubtasks to delete them too."],
                    "errors": {}
                }),
            ),
            DeleteAccess::RateLimited => HttpResponse {
                status: 429,
                headers: HashMap::from([("retry-after".to_string(), "30".to_string())]),
                body: String::new(),
            },
            DeleteAccess::ServerError => Self::reply(
                500,
                json!({"errorMessages": ["Internal server error"], "errors": {}}),
            ),
        }
    }

    fn search(&self, body: &Value) -> HttpResponse {
        let jql = body["jql"].as_str().unwrap_or_default();
        let required = jql
            .split("labels = \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .map(str::to_string)
            .collect::<Vec<_>>();
        let issues = self
            .issues
            .lock()
            .unwrap()
            .values()
            .filter(|issue| {
                let labels = issue["fields"]["labels"].as_array().unwrap();
                required.iter().all(|label| labels.contains(&json!(label)))
            })
            .cloned()
            .collect::<Vec<_>>();
        Self::reply(200, json!({"issues": issues, "isLast": true}))
    }
}

impl HttpTransport for FakeJira {
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
        let rest = url
            .strip_prefix("https://jira.test/rest/api/3/")
            .ok_or_else(|| format!("unexpected host: {url}"))?;
        let path = rest.split('?').next().unwrap_or(rest);
        if path == "search/jql" && method == "POST" {
            return Ok(self.search(body.unwrap()));
        }
        let Some(issue_path) = path.strip_prefix("issue/") else {
            return Ok(Self::reply(404, json!({"errorMessages": ["Not found"]})));
        };
        if let Some(key) = issue_path.strip_suffix("/comment") {
            return Ok(if self.exists(key) {
                Self::reply(
                    200,
                    json!({"startAt": 0, "maxResults": 100, "total": 0, "comments": []}),
                )
            } else {
                Self::missing()
            });
        }
        Ok(match method {
            "GET" => self
                .issues
                .lock()
                .unwrap()
                .get(issue_path)
                .map_or_else(Self::missing, |issue| Self::reply(200, issue.clone())),
            "PUT" => self.put(issue_path, body.unwrap()),
            "DELETE" => self.delete(issue_path),
            _ => Self::reply(405, json!({"errorMessages": ["Method not allowed"]})),
        })
    }
}

fn provider(fake: &Arc<FakeJira>) -> JiraProvider {
    JiraProvider::new(
        JiraConfig {
            connection_id: "jira-eng".into(),
            project_key: "ENG".into(),
            base_url: "https://jira.test".into(),
            email: "synthetic@example.com".into(),
            token: "fixture-token".into(),
            default: false,
        },
        fake.clone(),
    )
}

fn now() -> Timestamp {
    Timestamp::new("2026-11-25T00:00:00Z")
}

fn delete(jira: &JiraProvider, key: &str) -> ApiTicket {
    jira.update(
        key,
        now(),
        ProviderPatch {
            status: Some(Status::Deleted),
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn hs2_73z1r8_delete_trashes_every_workflow_category_and_restore_returns_it_exactly() {
    for (category, status, close_reason) in [
        ("new", Status::NotStarted, None),
        ("indeterminate", Status::Started, None),
        ("done", Status::Completed, Some(CloseReason::Completed)),
    ] {
        let fake = FakeJira::new(DeleteAccess::Allowed);
        fake.seed("ENG-1", category, &["customer"]);
        let jira = provider(&fake);

        let trashed = delete(&jira, "ENG-1");
        assert_eq!(trashed.status, Status::Deleted, "{category}");
        assert_eq!(trashed.tags, ["customer"], "the Trash label is never a tag");
        assert_eq!(trashed.close_reason, close_reason, "{category}");
        assert_eq!(fake.labels("ENG-1"), ["customer", TRASH_LABEL]);
        assert_eq!(
            fake.category("ENG-1"),
            category,
            "Delete never transitions the workflow"
        );
        assert_eq!(jira.get("ENG-1").unwrap().status, Status::Deleted);

        // Deleting again changes nothing.
        let again = delete(&jira, "ENG-1");
        assert_eq!(again.status, Status::Deleted);
        assert_eq!(fake.labels("ENG-1"), ["customer", TRASH_LABEL]);

        let restored = jira.restore("ENG-1", now()).unwrap();
        assert_eq!(restored.status, status, "{category}");
        assert_eq!(restored.close_reason, close_reason, "{category}");
        assert_eq!(restored.tags, ["customer"]);
        assert_eq!(fake.labels("ENG-1"), ["customer"]);
        assert_eq!(fake.category("ENG-1"), category);

        // A second restore names the reason rather than succeeding silently.
        let error = jira.restore("ENG-1", now()).unwrap_err();
        assert!(
            matches!(
                error,
                ProviderError::Operation(OpError::NotInTrash(found)) if found == status
            ),
            "{error:?}"
        );
    }
}

#[test]
fn hs2_73z1r8_restore_removes_only_the_trash_label_with_a_label_operation() {
    let fake = FakeJira::new(DeleteAccess::Allowed);
    fake.seed("ENG-2", "indeterminate", &["customer", TRASH_LABEL]);
    let jira = provider(&fake);
    let puts = fake.count("PUT");
    jira.restore("ENG-2", now()).unwrap();
    assert_eq!(fake.count("PUT"), puts + 1);
    let (url, body) = fake.last("PUT");
    assert_eq!(url, "https://jira.test/rest/api/3/issue/ENG-2");
    // `update.labels.remove` leaves any label another client added concurrently.
    assert_eq!(
        body,
        Some(json!({"update": {"labels": [{"remove": TRASH_LABEL}]}}))
    );
}

#[test]
fn hs2_73z1r8_edits_keep_trash_and_a_trash_tag_never_trashes() {
    let fake = FakeJira::new(DeleteAccess::Allowed);
    fake.seed("ENG-3", "new", &["customer", TRASH_LABEL]);
    fake.seed("ENG-4", "new", &["customer"]);
    let jira = provider(&fake);

    // A tag edit on a trashed issue keeps it in Trash.
    let edited = jira
        .update(
            "ENG-3",
            now(),
            ProviderPatch {
                tags: Some(vec!["triaged".into()]),
                title: Some("Renamed".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(edited.status, Status::Deleted);
    assert_eq!(edited.tags, ["triaged"]);
    assert_eq!(fake.labels("ENG-3"), [TRASH_LABEL, "triaged"]);

    // The Trash label passed as an ordinary tag is dropped, not used to trash the issue.
    let tagged = jira
        .update(
            "ENG-4",
            now(),
            ProviderPatch {
                tags: Some(vec!["customer".into(), TRASH_LABEL.into()]),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(tagged.status, Status::NotStarted);
    assert_eq!(fake.labels("ENG-4"), ["customer"]);
}

#[test]
fn hs2_73z1r8_other_status_changes_stay_unsupported_without_a_write() {
    let fake = FakeJira::new(DeleteAccess::Allowed);
    fake.seed("ENG-5", "new", &[]);
    fake.seed("ENG-6", "new", &[TRASH_LABEL]);
    let jira = provider(&fake);
    for (key, status) in [
        ("ENG-5", Status::Started),
        ("ENG-5", Status::Completed),
        ("ENG-6", Status::NotStarted),
        ("ENG-6", Status::Archive),
    ] {
        let error = jira
            .update(
                key,
                now(),
                ProviderPatch {
                    status: Some(status),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(
            matches!(
                error,
                ProviderError::Unsupported {
                    capability: "status transitions",
                    ..
                }
            ),
            "{key} {status:?}: {error:?}"
        );
    }
    assert_eq!(fake.count("PUT"), 0);
}

#[test]
fn hs2_73z1r8_queries_keep_trash_apart_from_open_and_queue_lists() {
    let fake = FakeJira::new(DeleteAccess::Allowed);
    fake.seed("ENG-1", "new", &[]);
    fake.seed("ENG-2", "indeterminate", &[TRASH_LABEL]);
    fake.seed("ENG-3", "done", &[TRASH_LABEL]);
    let jira = provider(&fake);
    let keys = |query: TicketQuery| {
        jira.query(&query)
            .unwrap()
            .into_iter()
            .map(|ticket| ticket.native_id)
            .collect::<Vec<_>>()
    };

    assert_eq!(
        keys(TicketQuery {
            status: Some(Status::Deleted),
            ..Default::default()
        }),
        ["ENG-2", "ENG-3"]
    );
    let (_, search) = fake.last("POST");
    assert!(
        search.unwrap()["jql"]
            .as_str()
            .unwrap()
            .contains("labels = \"hotsheet-trash\""),
        "the Trash read is pushed down to JQL"
    );
    assert_eq!(
        keys(TicketQuery {
            open_only: true,
            ..Default::default()
        }),
        ["ENG-1"],
        "a trashed issue in an open workflow status is not open"
    );
    assert_eq!(
        keys(TicketQuery {
            status: Some(Status::Started),
            ..Default::default()
        }),
        Vec::<String>::new()
    );
    assert_eq!(
        keys(TicketQuery {
            collection: Some(TicketCollection::Trash),
            ..Default::default()
        }),
        ["ENG-2", "ENG-3"]
    );
    for collection in [TicketCollection::Queue, TicketCollection::All] {
        assert_eq!(
            keys(TicketQuery {
                collection: Some(collection),
                ..Default::default()
            }),
            ["ENG-1"],
            "{collection:?}"
        );
    }
    let page = jira
        .query_page(
            &TicketQuery {
                collection: Some(TicketCollection::Trash),
                ..Default::default()
            },
            None,
            10,
        )
        .unwrap();
    assert_eq!(page.items.len(), 2);
    assert!(
        page.items
            .iter()
            .all(|ticket| ticket.status == Status::Deleted)
    );
}

#[test]
fn hs2_73z1r8_purge_deletes_only_trashed_issues_and_is_idempotent() {
    let fake = FakeJira::new(DeleteAccess::Allowed);
    fake.seed("ENG-1", "new", &[TRASH_LABEL]);
    fake.seed("ENG-2", "new", &[]);
    let jira = provider(&fake);

    assert_eq!(
        jira.purge_trashed("ENG-1").unwrap(),
        ProviderPurgeOutcome::Purged
    );
    assert!(!fake.exists("ENG-1"));
    let (url, _) = fake.last("DELETE");
    assert_eq!(
        url, "https://jira.test/rest/api/3/issue/ENG-1",
        "subtasks are never deleted along with the parent"
    );
    // Already gone: purged without another DELETE.
    let deletes = fake.count("DELETE");
    assert_eq!(
        jira.purge_trashed("ENG-1").unwrap(),
        ProviderPurgeOutcome::Purged
    );
    assert_eq!(fake.count("DELETE"), deletes);

    // Outside Trash: refused before any write.
    let error = jira.purge_trashed("ENG-2").unwrap_err();
    assert!(
        matches!(
            error,
            ProviderError::Operation(OpError::NotInTrash(Status::NotStarted))
        ),
        "{error:?}"
    );
    assert!(fake.exists("ENG-2"));
    assert_eq!(fake.count("DELETE"), deletes);

    // A malformed key never reaches Jira.
    assert!(matches!(
        jira.purge_trashed("ENG-1/../2"),
        Err(ProviderError::InvalidNativeId { .. })
    ));
}

#[test]
fn hs2_73z1r8_a_delete_racing_another_purge_counts_404_as_purged() {
    /// Answers the Trash read, then reports the issue gone on DELETE.
    struct Racing(Arc<FakeJira>);
    impl HttpTransport for Racing {
        fn request(
            &self,
            method: &str,
            url: &str,
            headers: &[(&str, String)],
            body: Option<&Value>,
        ) -> Result<HttpResponse, String> {
            if method == "DELETE" {
                return Ok(FakeJira::missing());
            }
            self.0.request(method, url, headers, body)
        }
    }
    let fake = FakeJira::new(DeleteAccess::Allowed);
    fake.seed("ENG-1", "done", &[TRASH_LABEL]);
    let jira = JiraProvider::new(
        JiraConfig {
            connection_id: "jira-eng".into(),
            project_key: "ENG".into(),
            base_url: "https://jira.test".into(),
            email: "synthetic@example.com".into(),
            token: "fixture-token".into(),
            default: false,
        },
        Arc::new(Racing(fake)),
    );
    assert_eq!(
        jira.purge_trashed("ENG-1").unwrap(),
        ProviderPurgeOutcome::Purged
    );
}

#[test]
fn hs2_73z1r8_purge_refusals_keep_the_issue_trashed_and_say_why() {
    for (access, reason, jira_text) in [
        (
            DeleteAccess::NoPermission,
            PURGE_NOT_PERMITTED,
            "You do not have permission to delete issues in this project.",
        ),
        (
            DeleteAccess::HasSubtasks,
            PURGE_HAS_CHILDREN,
            "The issue has subtasks.",
        ),
    ] {
        let fake = FakeJira::new(access);
        fake.seed("ENG-1", "done", &[TRASH_LABEL]);
        let jira = provider(&fake);
        let ProviderPurgeOutcome::Retained { message } = jira.purge_trashed("ENG-1").unwrap()
        else {
            panic!("{access:?} must be retained");
        };
        assert!(message.starts_with(reason), "{message}");
        assert!(message.contains(jira_text), "{message}");
        assert!(fake.exists("ENG-1"));
        assert_eq!(jira.get("ENG-1").unwrap().status, Status::Deleted);
    }
}

#[test]
fn hs2_73z1r8_rate_limits_and_server_errors_fail_instead_of_claiming_an_outcome() {
    let fake = FakeJira::new(DeleteAccess::RateLimited);
    fake.seed("ENG-1", "new", &[TRASH_LABEL]);
    let jira = provider(&fake);
    assert!(matches!(
        jira.purge_trashed("ENG-1"),
        Err(ProviderError::RateLimited {
            retry_after_seconds: Some(30),
            ..
        })
    ));
    *fake.access.lock().unwrap() = DeleteAccess::ServerError;
    assert!(matches!(
        jira.purge_trashed("ENG-1"),
        Err(ProviderError::Conflict { .. })
    ));
    assert!(fake.exists("ENG-1"));
}

#[test]
fn hs2_73z1r8_expiry_and_empty_trash_purge_through_the_shared_provider_path() {
    let fake = FakeJira::new(DeleteAccess::Allowed);
    fake.seed("ENG-1", "new", &[]);
    fake.seed("ENG-2", "done", &[]);
    fake.seed("ENG-3", "new", &[]);
    let jira = provider(&fake);
    delete(&jira, "ENG-1");
    delete(&jira, "ENG-2");
    // Jira's `+0000` offsets are read, so an old Trash ticket does reach retention.
    fake.set_updated("ENG-1", "2026-10-01T00:00:00.000+0000");

    let expired = purge_provider_trash(&jira, &now(), Some(30)).unwrap();
    assert_eq!(expired.purged, ["ENG-1"]);
    assert!(expired.retained.is_empty());
    assert!(!fake.exists("ENG-1") && fake.exists("ENG-2"));

    let emptied = purge_provider_trash(&jira, &now(), None).unwrap();
    assert_eq!(emptied.purged, ["ENG-2"]);
    assert!(fake.exists("ENG-3"), "a live issue is never purged");
    assert_eq!(
        purge_provider_trash(&jira, &now(), None).unwrap(),
        ProviderTrashPurge::default()
    );
}

#[test]
fn hs2_73z1r8_empty_trash_without_permission_reports_every_retained_ticket() {
    let fake = FakeJira::new(DeleteAccess::NoPermission);
    fake.seed("ENG-1", "new", &[TRASH_LABEL]);
    fake.seed("ENG-2", "done", &[TRASH_LABEL]);
    let jira = provider(&fake);
    let report = purge_provider_trash(&jira, &now(), None).unwrap();
    assert!(report.purged.is_empty());
    assert_eq!(
        report
            .retained
            .iter()
            .map(|ticket| ticket.qualified_id.as_str())
            .collect::<Vec<_>>(),
        ["jira-eng:ENG-1", "jira-eng:ENG-2"]
    );
    assert!(fake.exists("ENG-1") && fake.exists("ENG-2"));
}

/// Opt-in round trip against a real Jira project. There is no always-safe Jira target in
/// this repository's live-test configuration, so this only runs by hand against a throwaway
/// project: it creates an `[HS2 trash]` issue, trashes and restores it, then purges it.
/// Without the Delete Issues permission the purge must report the issue as retained.
#[test]
#[ignore = "writes a real Jira issue; set HOTSHEET_JIRA_LIVE_BASE_URL/PROJECT/EMAIL/TOKEN and HOTSHEET_JIRA_LIVE_WRITE=1"]
fn jira_live_trash_delete_restore_and_permission_checked_purge() {
    use hotsheet_ticketing::{MutationContext, ProviderDraft};
    let env = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name}"));
    assert_eq!(
        std::env::var("HOTSHEET_JIRA_LIVE_WRITE").ok().as_deref(),
        Some("1"),
        "set HOTSHEET_JIRA_LIVE_WRITE=1 to write to the live project"
    );
    let jira = JiraProvider::live(JiraConfig {
        connection_id: "jira-live".into(),
        project_key: env("HOTSHEET_JIRA_LIVE_PROJECT"),
        base_url: env("HOTSHEET_JIRA_LIVE_BASE_URL"),
        email: env("HOTSHEET_JIRA_LIVE_EMAIL"),
        token: env("HOTSHEET_JIRA_LIVE_TOKEN"),
        default: false,
    });
    let created = jira
        .create(
            MutationContext {
                now: now(),
                generated_id: hotsheet_model::Ulid::new(),
            },
            ProviderDraft {
                group: None,
                title: "[HS2 trash] delete-restore-purge".into(),
                category: std::env::var("HOTSHEET_JIRA_LIVE_ISSUE_TYPE")
                    .unwrap_or_else(|_| "Task".into()),
                priority: hotsheet_model::Priority::Default,
                status: Status::NotStarted,
                details: "Created by an opt-in Hot Sheet provider validation (HS2-73Z1R8).".into(),
                tags: vec!["hs2-benchmark".into()],
                up_next: false,
                blocked_by: vec![],
                transfer: None,
                created_by: None,
            },
        )
        .unwrap();
    let key = created.native_id.clone();
    let before = created.status;
    assert_eq!(delete(&jira, &key).status, Status::Deleted);
    assert_eq!(jira.get(&key).unwrap().status, Status::Deleted);
    let restored = jira.restore(&key, now()).unwrap();
    assert_eq!(restored.status, before);
    assert_eq!(restored.tags, ["hs2-benchmark"]);
    delete(&jira, &key);
    match jira.purge_trashed(&key).unwrap() {
        ProviderPurgeOutcome::Purged => {
            assert!(matches!(
                jira.get(&key),
                Err(ProviderError::NotFound { .. })
            ));
        }
        ProviderPurgeOutcome::Retained { message } => {
            assert!(message.starts_with(PURGE_NOT_PERMITTED), "{message}");
            assert_eq!(jira.get(&key).unwrap().status, Status::Deleted);
        }
    }
}

/// A git-backed destination connection and a registry holding it beside the Jira source.
fn move_registry(
    fake: &Arc<FakeJira>,
) -> (
    tempfile::TempDir,
    hotsheet_ticketing::FsStore,
    hotsheet_ticketing::ProviderRegistry,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = hotsheet_ticketing::FsStore::init(
        dir.path(),
        &hotsheet_ticketing::StoreMetadata::new("DST"),
    )
    .unwrap();
    let registry = hotsheet_ticketing::ProviderRegistry::default();
    registry.register(Arc::new(provider(fake))).unwrap();
    registry
        .register(Arc::new(hotsheet_ticketing::GitProvider::new(
            "local",
            store.clone(),
        )))
        .unwrap();
    (dir, store, registry)
}

fn jira_source(key: &str) -> hotsheet_ticketing::TicketRef {
    hotsheet_ticketing::TicketRef {
        connection_id: "jira-eng".into(),
        native_id: key.into(),
    }
}

fn jira_source_status(registry: &hotsheet_ticketing::ProviderRegistry) -> Status {
    registry
        .get("jira-eng")
        .unwrap()
        .get("ENG-7")
        .unwrap()
        .status
}

#[test]
fn hs2_dag8p8_moving_out_of_jira_trashes_the_source_with_exactly_one_destination() {
    // Jira cannot close (`close: false`), so every move used to create the destination
    // and then fail on close, leaving a duplicate on each attempt.
    let fake = FakeJira::new(DeleteAccess::Allowed);
    fake.seed("ENG-7", "indeterminate", &["backend"]);
    let (_dir, store, registry) = move_registry(&fake);
    for _ in 0..2 {
        let moved = hotsheet_ticketing::move_between(
            &registry,
            jira_source("ENG-7"),
            "local",
            "move-eng-7",
            now(),
        )
        .unwrap();
        assert!(moved.moved);
        assert_eq!(moved.source.qualified(), "jira-eng:ENG-7");
        assert_eq!(store.list_tickets().unwrap().len(), 1);
        let copied = registry
            .get("local")
            .unwrap()
            .get(&moved.destination.native_id)
            .unwrap();
        assert_eq!(copied.transferred_from.as_deref(), Some("jira-eng:ENG-7"));
        assert_eq!(copied.tags, ["backend"]);
        // Trashed by its label alone: the workflow status is untouched and no
        // transition is ever requested.
        assert_eq!(fake.labels("ENG-7"), ["backend", TRASH_LABEL]);
        assert_eq!(fake.category("ENG-7"), "indeterminate");
        assert_eq!(jira_source_status(&registry), Status::Deleted);
    }
    assert!(
        !fake
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|(_, url, _)| url.contains("/transitions")),
        "a move must not transition the Jira issue"
    );
}

#[test]
fn hs2_dag8p8_a_failed_jira_trash_reports_the_destination_and_a_retry_finishes_the_move() {
    let fake = FakeJira::new(DeleteAccess::Allowed);
    fake.seed("ENG-7", "new", &[]);
    let (_dir, store, registry) = move_registry(&fake);
    *fake.failing_puts.lock().unwrap() = 1;
    let error = hotsheet_ticketing::move_between(
        &registry,
        jira_source("ENG-7"),
        "local",
        "move-eng-7",
        now(),
    )
    .unwrap_err();
    let hotsheet_ticketing::TransferError::SourceCloseFailed { destination, .. } = &error else {
        panic!("expected a reported partial move, got {error}")
    };
    assert!(destination.starts_with("local:"), "{destination}");
    assert!(error.to_string().contains("same operation id"), "{error}");
    // Recoverable: one destination, and the source is still live and untrashed.
    assert_eq!(store.list_tickets().unwrap().len(), 1);
    assert!(fake.labels("ENG-7").is_empty());
    assert_eq!(jira_source_status(&registry), Status::NotStarted);

    let recovered = hotsheet_ticketing::move_between(
        &registry,
        jira_source("ENG-7"),
        "local",
        "move-eng-7",
        now(),
    )
    .unwrap();
    assert!(recovered.moved);
    assert_eq!(&recovered.destination.qualified(), destination);
    assert_eq!(store.list_tickets().unwrap().len(), 1);
    assert_eq!(fake.labels("ENG-7"), [TRASH_LABEL]);
    assert_eq!(jira_source_status(&registry), Status::Deleted);
}
