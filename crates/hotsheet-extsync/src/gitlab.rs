use std::sync::Arc;

use hotsheet_model::{CloseReason, NoteKind, Priority, ReviewRequest, Status, Timestamp};
use hotsheet_ticketing::{
    ApiNote, ApiTicket, MutationContext, ProviderCapabilities, ProviderConnection,
    ProviderDescriptor, ProviderDraft, ProviderError, ProviderPatch, SortKey, TicketProvider,
    TicketQuery,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::github::{GitHubTransport as HttpTransport, HttpResponse, UreqGitHubTransport};

#[derive(Debug, Clone)]
pub struct GitLabConfig {
    pub connection_id: String,
    pub project: String,
    pub api_base: String,
    pub token: String,
    pub default: bool,
}

impl GitLabConfig {
    pub fn from_connection(
        connection: &ProviderConnection,
        token: impl Into<String>,
    ) -> Result<Self, ProviderError> {
        if connection.provider != "gitlab" || !connection.locator.contains('/') {
            return Err(ProviderError::Conflict {
                ticket: connection.id.clone(),
                message: "GitLab locator must be a namespace/project path".into(),
            });
        }
        Ok(Self {
            connection_id: connection.id.clone(),
            project: connection.locator.clone(),
            api_base: connection
                .settings
                .get("api_base")
                .and_then(Value::as_str)
                .unwrap_or("https://gitlab.com/api/v4")
                .into(),
            token: token.into(),
            default: connection.default,
        })
    }
}

#[derive(Clone)]
pub struct GitLabProvider {
    config: GitLabConfig,
    transport: Arc<dyn HttpTransport>,
}

impl GitLabProvider {
    pub fn new(config: GitLabConfig, transport: Arc<dyn HttpTransport>) -> Self {
        Self { config, transport }
    }

    pub fn live(config: GitLabConfig) -> Self {
        Self::new(config, Arc::new(UreqGitHubTransport))
    }

    fn endpoint(&self, suffix: &str) -> String {
        format!(
            "{}/projects/{}/{}",
            self.config.api_base.trim_end_matches('/'),
            encode_path(&self.config.project),
            suffix.trim_start_matches('/')
        )
    }

    fn request(
        &self,
        method: &str,
        url: &str,
        body: Option<&Value>,
    ) -> Result<HttpResponse, ProviderError> {
        let headers = [
            ("PRIVATE-TOKEN", self.config.token.clone()),
            ("User-Agent", "hotsheet2".into()),
        ];
        let response = self
            .transport
            .request(method, url, &headers, body)
            .map_err(|message| ProviderError::Conflict {
                ticket: self.config.connection_id.clone(),
                message,
            })?;
        match response.status {
            200..=299 => Ok(response),
            401 => Err(ProviderError::Authentication {
                connection_id: self.config.connection_id.clone(),
                message: response_message(&response.body),
            }),
            403 if response.headers.contains_key("retry-after")
                || response
                    .headers
                    .get("ratelimit-remaining")
                    .is_some_and(|value| value == "0") =>
            {
                Err(ProviderError::RateLimited {
                    connection_id: self.config.connection_id.clone(),
                    retry_after_seconds: response
                        .headers
                        .get("retry-after")
                        .and_then(|value| value.parse().ok()),
                })
            }
            403 => Err(ProviderError::Authentication {
                connection_id: self.config.connection_id.clone(),
                message: response_message(&response.body),
            }),
            429 => Err(ProviderError::RateLimited {
                connection_id: self.config.connection_id.clone(),
                retry_after_seconds: response
                    .headers
                    .get("retry-after")
                    .and_then(|value| value.parse().ok()),
            }),
            404 => Err(ProviderError::NotFound {
                connection_id: self.config.connection_id.clone(),
                native_id: url.rsplit('/').next().unwrap_or(url).into(),
            }),
            _ => Err(ProviderError::Conflict {
                ticket: url.into(),
                message: response_message(&response.body),
            }),
        }
    }

    fn json<T: for<'de> Deserialize<'de>>(
        &self,
        response: HttpResponse,
    ) -> Result<T, ProviderError> {
        serde_json::from_str(&response.body).map_err(|error| ProviderError::Conflict {
            ticket: self.config.connection_id.clone(),
            message: format!("invalid GitLab response: {error}"),
        })
    }

    fn issue(&self, native_id: &str) -> Result<GitLabIssue, ProviderError> {
        validate_iid(native_id)?;
        let response = self.request("GET", &self.endpoint(&format!("issues/{native_id}")), None)?;
        self.json(response)
    }

    fn notes(&self, native_id: &str) -> Result<Vec<GitLabNote>, ProviderError> {
        let response = self.request(
            "GET",
            &self.endpoint(&format!("issues/{native_id}/notes?per_page=100")),
            None,
        )?;
        self.json(response)
    }

    fn list_issues(&self, updated_after: Option<&str>) -> Result<Vec<GitLabIssue>, ProviderError> {
        let mut page = 1;
        let mut issues = Vec::new();
        loop {
            let mut url = self.endpoint(&format!("issues?scope=all&per_page=100&page={page}"));
            if let Some(after) = updated_after {
                url.push_str("&updated_after=");
                url.push_str(after);
            }
            let response = self.request("GET", &url, None)?;
            let next = response
                .headers
                .get("x-next-page")
                .filter(|value| !value.is_empty())
                .and_then(|value| value.parse::<u32>().ok());
            issues.extend(self.json::<Vec<GitLabIssue>>(response)?);
            let Some(next) = next else { break };
            page = next;
        }
        Ok(issues)
    }

    fn ticket(&self, issue: GitLabIssue, notes: Vec<GitLabNote>) -> ApiTicket {
        let category = mapped_label(&issue.labels, "category:").unwrap_or_else(|| "issue".into());
        let priority = mapped_label(&issue.labels, "priority:")
            .as_deref()
            .and_then(parse_priority)
            .unwrap_or_default();
        let status = if issue.state == "closed" {
            Status::Completed
        } else if issue.labels.iter().any(|label| label == "status:started") {
            Status::Started
        } else if issue.labels.iter().any(|label| label == "status:backlog") {
            Status::Backlog
        } else {
            Status::NotStarted
        };
        let body = issue.description.unwrap_or_default();
        let native_id = issue.iid.to_string();
        ApiTicket {
            connection_id: self.config.connection_id.clone(),
            native_id: native_id.clone(),
            qualified_id: format!("{}:{native_id}", self.config.connection_id),
            native_url: Some(issue.web_url),
            concurrency_token: Some(issue.updated_at.clone()),
            id: native_id.clone(),
            slug: format!("{}#{native_id}", self.config.project),
            title: issue.title,
            details: strip_transfer(&body),
            category,
            priority,
            status,
            up_next: false,
            tags: issue
                .labels
                .into_iter()
                .filter(|label| {
                    !label.starts_with("category:")
                        && !label.starts_with("priority:")
                        && !label.starts_with("status:")
                })
                .collect(),
            blocked_by: vec![],
            blocked_reason: None,
            created_at: issue.created_at,
            updated_at: issue.updated_at,
            completed_at: issue.closed_at.clone(),
            verified_at: None,
            closed_at: issue.closed_at,
            close_reason: (issue.state == "closed").then_some(CloseReason::Completed),
            duplicate_of: None,
            copied_from: None,
            transfer_operation_id: marker(&body, "operation"),
            transferred_from: marker(&body, "source"),
            moved_to_store: None,
            moved_at: None,
            claimed_by: None,
            worker_label: None,
            claim_count: 0,
            assignees: issue
                .assignees
                .into_iter()
                .map(|user| user.username)
                .collect(),
            review_requests: vec![],
            schema: 1,
            notes: notes
                .into_iter()
                .filter(|note| !note.system)
                .map(|note| ApiNote {
                    id: note.id.to_string(),
                    kind: NoteKind::Regular,
                    created_at: note.created_at.clone(),
                    edited_at: note.updated_at.unwrap_or(note.created_at),
                    text: strip_note(&note.body),
                })
                .collect(),
            auto_context: vec![],
        }
    }

    fn unsupported<T>(&self, capability: &'static str) -> Result<T, ProviderError> {
        Err(ProviderError::Unsupported {
            connection_id: self.config.connection_id.clone(),
            capability,
        })
    }
}

impl TicketProvider for GitLabProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            connection_id: self.config.connection_id.clone(),
            provider: "gitlab".into(),
            display_name: format!("GitLab {}", self.config.project),
            locator: self.config.project.clone(),
            default: self.config.default,
            capabilities: capabilities(),
        }
    }

    fn query(&self, query: &TicketQuery) -> Result<Vec<ApiTicket>, ProviderError> {
        if query.text.is_some()
            || query.review_requested.is_some()
            || query.review_by.is_some()
            || query.claimed.is_some()
            || query.blocked.is_some()
            || query.page_after.is_some()
            || query.up_next_only
        {
            return self.unsupported("requested query filter");
        }
        let mut tickets = self
            .list_issues(query.updated_after.as_deref())?
            .into_iter()
            .map(|issue| self.ticket(issue, vec![]))
            .filter(|ticket| query.status.is_none_or(|value| ticket.status == value))
            .filter(|ticket| query.priority.is_none_or(|value| ticket.priority == value))
            .filter(|ticket| {
                query
                    .category
                    .as_deref()
                    .is_none_or(|value| ticket.category == value)
            })
            .filter(|ticket| query.tags.iter().all(|tag| ticket.tags.contains(tag)))
            .filter(|ticket| !query.open_only || ticket.close_reason.is_none())
            .filter(|ticket| {
                query
                    .close_reason
                    .is_none_or(|value| ticket.close_reason == Some(value))
            })
            .filter(|ticket| {
                query
                    .closed
                    .is_none_or(|value| ticket.close_reason.is_some() == value)
            })
            .filter(|ticket| {
                query
                    .assignee
                    .as_deref()
                    .is_none_or(|value| ticket.assignees.iter().any(|a| a == value))
            })
            .filter(|ticket| {
                query
                    .created_after
                    .as_deref()
                    .is_none_or(|value| ticket.created_at.as_str() >= value)
            })
            .filter(|ticket| {
                query
                    .created_before
                    .as_deref()
                    .is_none_or(|value| ticket.created_at.as_str() <= value)
            })
            .filter(|ticket| {
                query
                    .updated_after
                    .as_deref()
                    .is_none_or(|value| ticket.updated_at.as_str() >= value)
            })
            .filter(|ticket| {
                query
                    .updated_before
                    .as_deref()
                    .is_none_or(|value| ticket.updated_at.as_str() <= value)
            })
            .collect::<Vec<_>>();
        tickets.sort_by(|a, b| match query.sort {
            SortKey::Id => a.native_id.cmp(&b.native_id),
            SortKey::Created => a.created_at.cmp(&b.created_at),
            SortKey::Updated => a.updated_at.cmp(&b.updated_at),
            SortKey::Priority => priority_rank(a.priority).cmp(&priority_rank(b.priority)),
            SortKey::Status => format!("{:?}", a.status).cmp(&format!("{:?}", b.status)),
            SortKey::Title => a.title.cmp(&b.title),
        });
        if let Some(limit) = query.limit {
            tickets.truncate(limit);
        }
        Ok(tickets)
    }

    fn find_transfer(&self, operation_id: &str) -> Result<Option<ApiTicket>, ProviderError> {
        Ok(self
            .list_issues(None)?
            .into_iter()
            .find(|issue| {
                marker(&issue.description.clone().unwrap_or_default(), "operation").as_deref()
                    == Some(operation_id)
            })
            .map(|issue| self.ticket(issue, vec![])))
    }

    fn get(&self, native_id: &str) -> Result<ApiTicket, ProviderError> {
        Ok(self.ticket(self.issue(native_id)?, self.notes(native_id)?))
    }

    fn create(&self, _: MutationContext, draft: ProviderDraft) -> Result<ApiTicket, ProviderError> {
        if !draft.blocked_by.is_empty() {
            return self.unsupported("dependencies");
        }
        if let Some(transfer) = &draft.transfer
            && let Some(existing) = self.find_transfer(&transfer.operation_id)?
        {
            if existing.transferred_from.as_deref() == Some(&transfer.source.qualified()) {
                return Ok(existing);
            }
            return Err(ProviderError::Conflict {
                ticket: transfer.operation_id.clone(),
                message: "operation id belongs to another source".into(),
            });
        }
        let mut description = draft.details;
        if let Some(transfer) = draft.transfer {
            description.push_str(&format!(
                "\n\n<!-- hotsheet-transfer {} -->",
                json!({"operation":transfer.operation_id,"source":transfer.source.qualified()})
            ));
        }
        let response = self.request(
            "POST",
            &self.endpoint("issues"),
            Some(&json!({
                "title":draft.title,"description":description,
                "labels":mapped_labels(&draft.category, draft.priority, &draft.tags, None).join(",")
            })),
        )?;
        Ok(self.ticket(self.json(response)?, vec![]))
    }

    fn update(
        &self,
        native_id: &str,
        _: Timestamp,
        patch: ProviderPatch,
    ) -> Result<ApiTicket, ProviderError> {
        let current = self.issue(native_id)?;
        if patch
            .expected_token
            .as_deref()
            .is_some_and(|value| value != current.updated_at)
        {
            return Err(ProviderError::Conflict {
                ticket: format!("{}:{native_id}", self.config.connection_id),
                message: "issue changed since it was read".into(),
            });
        }
        if patch
            .blocked_by
            .as_ref()
            .is_some_and(|value| !value.is_empty())
        {
            return self.unsupported("dependencies");
        }
        let normalized = self.ticket(current.clone(), vec![]);
        let category = patch.category.unwrap_or(normalized.category);
        let priority = patch.priority.unwrap_or(normalized.priority);
        let tags = patch.tags.unwrap_or(normalized.tags);
        let status = patch.status.unwrap_or(normalized.status);
        let mut description = patch
            .details
            .unwrap_or_else(|| current.description.clone().unwrap_or_default());
        if let Some(suffix) = transfer_suffix(&current.description)
            && !description.contains("<!-- hotsheet-transfer ")
        {
            description.push_str(&suffix);
        }
        let response = self.request("PUT", &self.endpoint(&format!("issues/{native_id}")), Some(&json!({
            "title":patch.title.unwrap_or(current.title), "description":description,
            "labels":mapped_labels(&category, priority, &tags, Some(status)).join(","),
            "state_event": if matches!(status, Status::Completed | Status::Verified | Status::Archive | Status::Deleted) { "close" } else { "reopen" }
        })))?;
        Ok(self.ticket(self.json(response)?, vec![]))
    }

    fn add_note(
        &self,
        native_id: &str,
        ctx: MutationContext,
        _: NoteKind,
        text: String,
    ) -> Result<ApiTicket, ProviderError> {
        let note_marker = format!("<!-- hotsheet-note-id:{} -->", ctx.generated_id);
        if self
            .notes(native_id)?
            .iter()
            .any(|note| note.body.contains(&note_marker))
        {
            return self.get(native_id);
        }
        self.request(
            "POST",
            &self.endpoint(&format!("issues/{native_id}/notes")),
            Some(&json!({"body":format!("{text}\n\n{note_marker}")})),
        )?;
        self.get(native_id)
    }

    fn close(
        &self,
        native_id: &str,
        _: Timestamp,
        _: CloseReason,
        _: Option<String>,
    ) -> Result<ApiTicket, ProviderError> {
        let response = self.request(
            "PUT",
            &self.endpoint(&format!("issues/{native_id}")),
            Some(&json!({"state_event":"close"})),
        )?;
        Ok(self.ticket(self.json(response)?, vec![]))
    }

    fn assign(
        &self,
        native_id: &str,
        _: Timestamp,
        assignees: Option<Vec<String>>,
        reviews: Vec<ReviewRequest>,
    ) -> Result<ApiTicket, ProviderError> {
        let _ = (native_id, assignees, reviews);
        self.unsupported("assignment requires provider-native numeric user ids")
    }

    fn claim_next(
        &self,
        _: Timestamp,
        _: Timestamp,
        _: &str,
        _: Option<String>,
    ) -> Result<Option<ApiTicket>, ProviderError> {
        self.unsupported("claims")
    }
    fn release(&self, _: &str, _: Timestamp, _: &str, _: bool) -> Result<ApiTicket, ProviderError> {
        self.unsupported("claims")
    }
    fn renew(
        &self,
        _: &str,
        _: Timestamp,
        _: Timestamp,
        _: &str,
    ) -> Result<ApiTicket, ProviderError> {
        self.unsupported("claims")
    }
}

#[derive(Debug, Clone, Deserialize)]
struct GitLabIssue {
    iid: u64,
    title: String,
    description: Option<String>,
    state: String,
    web_url: String,
    created_at: String,
    updated_at: String,
    closed_at: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    assignees: Vec<GitLabUser>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitLabUser {
    username: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GitLabNote {
    id: u64,
    body: String,
    created_at: String,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    system: bool,
}

fn capabilities() -> ProviderCapabilities {
    ProviderCapabilities {
        create: true,
        update: true,
        close: true,
        notes: true,
        attachments: false,
        assignment: false,
        review_requests: false,
        dependencies: false,
        up_next: false,
        close_reasons: false,
        claims: false,
        atomic_batch: false,
        offline_mutation: false,
        history: true,
        watch: true,
        provider_idempotency: false,
        query_fields: [
            "status",
            "priority",
            "category",
            "tags",
            "assignee",
            "close_reason",
            "closed",
            "created_at",
            "updated_at",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
    }
}

fn validate_iid(value: &str) -> Result<(), ProviderError> {
    value
        .parse::<u64>()
        .map(|_| ())
        .map_err(|_| ProviderError::InvalidNativeId(value.into()))
}

fn encode_path(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn response_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| value.get("message")?.as_str().map(str::to_string))
        .unwrap_or_else(|| body.into())
}

fn parse_priority(value: &str) -> Option<Priority> {
    match value {
        "lowest" => Some(Priority::Lowest),
        "low" => Some(Priority::Low),
        "default" | "medium" => Some(Priority::Default),
        "high" => Some(Priority::High),
        "highest" | "critical" => Some(Priority::Highest),
        _ => None,
    }
}
fn priority_name(value: Priority) -> &'static str {
    match value {
        Priority::Lowest => "lowest",
        Priority::Low => "low",
        Priority::Default => "default",
        Priority::High => "high",
        Priority::Highest => "highest",
    }
}
fn priority_rank(value: Priority) -> u8 {
    match value {
        Priority::Highest => 0,
        Priority::High => 1,
        Priority::Default => 2,
        Priority::Low => 3,
        Priority::Lowest => 4,
    }
}
fn mapped_label(labels: &[String], prefix: &str) -> Option<String> {
    labels
        .iter()
        .find_map(|label| label.strip_prefix(prefix).map(str::to_string))
}
fn mapped_labels(
    category: &str,
    priority: Priority,
    tags: &[String],
    status: Option<Status>,
) -> Vec<String> {
    let mut labels = tags.to_vec();
    labels.push(format!("category:{category}"));
    labels.push(format!("priority:{}", priority_name(priority)));
    if status == Some(Status::Started) {
        labels.push("status:started".into())
    }
    if status == Some(Status::Backlog) {
        labels.push("status:backlog".into())
    }
    labels.sort();
    labels.dedup();
    labels
}
fn marker(body: &str, field: &str) -> Option<String> {
    let value = body
        .split("<!-- hotsheet-transfer ")
        .nth(1)?
        .split(" -->")
        .next()?;
    serde_json::from_str::<Value>(value)
        .ok()?
        .get(field)?
        .as_str()
        .map(str::to_string)
}
fn strip_transfer(body: &str) -> String {
    body.split("\n\n<!-- hotsheet-transfer ")
        .next()
        .unwrap_or(body)
        .into()
}
fn transfer_suffix(body: &Option<String>) -> Option<String> {
    Some(format!(
        "\n\n<!-- hotsheet-transfer {}",
        body.as_deref()?
            .split("\n\n<!-- hotsheet-transfer ")
            .nth(1)?
    ))
}
fn strip_note(body: &str) -> String {
    body.split("\n\n<!-- hotsheet-note-id:")
        .next()
        .unwrap_or(body)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;

    type RecordedRequest = (String, String, Vec<(String, String)>);

    #[derive(Default)]
    struct Fake {
        responses: Mutex<VecDeque<HttpResponse>>,
        requests: Mutex<Vec<RecordedRequest>>,
    }
    impl HttpTransport for Fake {
        fn request(
            &self,
            method: &str,
            url: &str,
            headers: &[(&str, String)],
            _: Option<&Value>,
        ) -> Result<HttpResponse, String> {
            self.requests.lock().unwrap().push((
                method.into(),
                url.into(),
                headers
                    .iter()
                    .map(|(k, v)| ((*k).into(), v.clone()))
                    .collect(),
            ));
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "unexpected request".into())
        }
    }
    fn response(status: u16, body: Value) -> HttpResponse {
        HttpResponse {
            status,
            headers: HashMap::new(),
            body: body.to_string(),
        }
    }
    fn issue(iid: u64, title: &str) -> Value {
        json!({"iid":iid,"title":title,"description":"body","state":"opened","web_url":format!("https://gitlab.test/acme/repo/-/issues/{iid}"),"created_at":"2026-08-26T00:00:00Z","updated_at":"2026-08-26T00:01:00Z","closed_at":null,"labels":["category:bug","priority:high","customer"],"assignees":[{"username":"dev"}]})
    }
    fn provider(fake: Arc<Fake>) -> GitLabProvider {
        GitLabProvider::new(
            GitLabConfig {
                connection_id: "gitlab-main".into(),
                project: "acme/repo".into(),
                api_base: "https://gitlab.test/api/v4".into(),
                token: "token".into(),
                default: false,
            },
            fake,
        )
    }

    #[test]
    fn maps_and_paginates_gitlab_issues_with_private_token() {
        let mut first = response(200, json!([issue(1, "one")]));
        first.headers.insert("x-next-page".into(), "2".into());
        let fake = Arc::new(Fake {
            responses: Mutex::new(vec![first, response(200, json!([issue(2, "two")]))].into()),
            ..Default::default()
        });
        let tickets = provider(fake.clone())
            .query(&TicketQuery::default())
            .unwrap();
        assert_eq!(
            tickets
                .iter()
                .map(|t| t.qualified_id.as_str())
                .collect::<Vec<_>>(),
            ["gitlab-main:1", "gitlab-main:2"]
        );
        assert_eq!(tickets[0].category, "bug");
        assert_eq!(tickets[0].priority, Priority::High);
        assert_eq!(tickets[0].tags, ["customer"]);
        let requests = fake.requests.lock().unwrap();
        assert!(requests[0].1.contains("projects/acme%2Frepo/issues"));
        assert!(
            requests[0]
                .2
                .iter()
                .any(|(k, v)| k == "PRIVATE-TOKEN" && v == "token")
        );
    }

    #[test]
    fn gitlab_concurrency_and_rate_limit_are_typed() {
        let fake = Arc::new(Fake {
            responses: Mutex::new(vec![response(200, issue(9, "remote"))].into()),
            ..Default::default()
        });
        let error = provider(fake)
            .update(
                "9",
                Timestamp::new("x"),
                ProviderPatch {
                    expected_token: Some("stale".into()),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(error, ProviderError::Conflict { .. }));
        let mut limited = response(429, json!({"message":"slow down"}));
        limited.headers.insert("retry-after".into(), "12".into());
        let fake = Arc::new(Fake {
            responses: Mutex::new(vec![limited].into()),
            ..Default::default()
        });
        assert!(matches!(
            provider(fake).get("1"),
            Err(ProviderError::RateLimited {
                retry_after_seconds: Some(12),
                ..
            })
        ));

        let fake = Arc::new(Fake {
            responses: Mutex::new(vec![response(403, json!({"message":"forbidden"}))].into()),
            ..Default::default()
        });
        assert!(matches!(
            provider(fake).get("1"),
            Err(ProviderError::Authentication { .. })
        ));
    }

    #[test]
    fn gitlab_applies_date_bounds_after_incremental_fetch() {
        let fake = Arc::new(Fake {
            responses: Mutex::new(
                vec![response(200, json!([issue(1, "one"), issue(2, "two")]))].into(),
            ),
            ..Default::default()
        });
        let mut second = issue(2, "two");
        second["created_at"] = json!("2026-08-27T00:00:00Z");
        second["updated_at"] = json!("2026-08-27T00:01:00Z");
        fake.responses.lock().unwrap().clear();
        fake.responses
            .lock()
            .unwrap()
            .push_back(response(200, json!([issue(1, "one"), second])));
        let tickets = provider(fake)
            .query(&TicketQuery {
                created_before: Some("2026-08-26T23:59:59Z".into()),
                updated_before: Some("2026-08-26T23:59:59Z".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            tickets
                .iter()
                .map(|ticket| ticket.native_id.as_str())
                .collect::<Vec<_>>(),
            ["1"]
        );
    }

    #[test]
    fn gitlab_declares_unsupported_assignment_and_dependencies() {
        let fake = Arc::new(Fake::default());
        let provider = provider(fake);
        assert!(!provider.descriptor().capabilities.assignment);
        assert!(matches!(
            provider.assign("1", Timestamp::new("x"), Some(vec!["dev".into()]), vec![]),
            Err(ProviderError::Unsupported { .. })
        ));
    }

    #[test]
    #[ignore = "reads a real GitLab project; set HOTSHEET_GITLAB_LIVE_PROJECT and HOTSHEET_GITLAB_LIVE_TOKEN"]
    fn gitlab_live_contract_drift() {
        let connection = ProviderConnection {
            id: "gitlab-live".into(),
            provider: "gitlab".into(),
            locator: std::env::var("HOTSHEET_GITLAB_LIVE_PROJECT").expect("live project"),
            name: None,
            default: false,
            settings: json!({"api_base":std::env::var("HOTSHEET_GITLAB_LIVE_API_BASE").unwrap_or_else(|_|"https://gitlab.com/api/v4".into())}),
        };
        let provider = GitLabProvider::live(
            GitLabConfig::from_connection(
                &connection,
                std::env::var("HOTSHEET_GITLAB_LIVE_TOKEN").expect("live token"),
            )
            .unwrap(),
        );
        provider
            .query(&TicketQuery {
                limit: Some(1),
                ..Default::default()
            })
            .unwrap();
    }
}
