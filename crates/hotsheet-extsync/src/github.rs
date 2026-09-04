use std::collections::HashMap;
use std::sync::Arc;

use hotsheet_model::{CloseReason, NoteKind, Priority, ReviewRequest, Status, Timestamp};
use hotsheet_ticketing::{
    ApiNote, ApiTicket, MutationContext, ProviderCapabilities, ProviderConnection,
    ProviderDescriptor, ProviderDraft, ProviderError, ProviderPatch, SortKey, TicketProvider,
    TicketQuery,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const API_VERSION: &str = "2022-11-28";

#[derive(Debug, Clone)]
pub struct GitHubConfig {
    pub connection_id: String,
    pub repository: String,
    pub api_base: String,
    pub token: String,
    pub default: bool,
}

impl GitHubConfig {
    pub fn new(
        connection_id: impl Into<String>,
        repository: impl Into<String>,
        token: impl Into<String>,
    ) -> Self {
        Self {
            connection_id: connection_id.into(),
            repository: repository.into(),
            api_base: "https://api.github.com".into(),
            token: token.into(),
            default: false,
        }
    }

    pub fn from_connection(
        connection: &ProviderConnection,
        token: impl Into<String>,
    ) -> Result<Self, ProviderError> {
        if connection.provider != "github" || !connection.locator.contains('/') {
            return Err(ProviderError::Conflict {
                ticket: connection.id.clone(),
                message: "GitHub locator must be 'owner/repository'".into(),
            });
        }
        Ok(Self {
            connection_id: connection.id.clone(),
            repository: connection.locator.clone(),
            api_base: connection
                .settings
                .get("api_base")
                .and_then(Value::as_str)
                .unwrap_or("https://api.github.com")
                .into(),
            token: token.into(),
            default: connection.default,
        })
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

pub trait GitHubTransport: Send + Sync {
    fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<HttpResponse, String>;
}

/// Normalized invalidation emitted after the host verifies a GitHub webhook signature.
/// The provider intentionally returns an identity/invalidation, not a mirrored payload;
/// consumers re-read the authoritative issue through [`TicketProvider::get`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubWebhook {
    pub action: String,
    pub native_id: String,
}

pub fn parse_webhook(event: &str, payload: &[u8]) -> Result<Option<GitHubWebhook>, ProviderError> {
    if !matches!(event, "issues" | "issue_comment") {
        return Ok(None);
    }
    let value: Value =
        serde_json::from_slice(payload).map_err(|error| ProviderError::Conflict {
            ticket: "github-webhook".into(),
            message: format!("invalid webhook payload: {error}"),
        })?;
    let number = value
        .get("issue")
        .and_then(|issue| issue.get("number"))
        .and_then(Value::as_u64)
        .ok_or_else(|| ProviderError::Conflict {
            ticket: "github-webhook".into(),
            message: "webhook has no issue.number".into(),
        })?;
    Ok(Some(GitHubWebhook {
        action: value
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("changed")
            .into(),
        native_id: number.to_string(),
    }))
}

#[derive(Debug, Default)]
pub struct UreqGitHubTransport;

impl GitHubTransport for UreqGitHubTransport {
    fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<HttpResponse, String> {
        let mut request = ureq::request(method, url);
        for (name, value) in headers {
            request = request.set(name, value);
        }
        let result = match body {
            Some(body) => request
                .set("Content-Type", "application/json")
                .send_string(&body.to_string()),
            None => request.call(),
        };
        let response = match result {
            Ok(response) => response,
            Err(ureq::Error::Status(_, response)) => response,
            Err(error) => return Err(error.to_string()),
        };
        let status = response.status();
        let mut response_headers = HashMap::new();
        for name in response.headers_names() {
            if let Some(value) = response.header(&name) {
                response_headers.insert(name.to_ascii_lowercase(), value.to_string());
            }
        }
        let body = response.into_string().map_err(|e| e.to_string())?;
        Ok(HttpResponse {
            status,
            headers: response_headers,
            body,
        })
    }
}

#[derive(Clone)]
pub struct GitHubProvider {
    config: GitHubConfig,
    transport: Arc<dyn GitHubTransport>,
}

impl GitHubProvider {
    pub fn new(config: GitHubConfig, transport: Arc<dyn GitHubTransport>) -> Self {
        Self { config, transport }
    }

    pub fn live(config: GitHubConfig) -> Self {
        Self::new(config, Arc::new(UreqGitHubTransport))
    }

    fn endpoint(&self, suffix: &str) -> String {
        format!(
            "{}/repos/{}/{}",
            self.config.api_base.trim_end_matches('/'),
            self.config.repository,
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
            ("Accept", "application/vnd.github+json".into()),
            ("Authorization", format!("Bearer {}", self.config.token)),
            ("X-GitHub-Api-Version", API_VERSION.into()),
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
            401 | 403
                if response
                    .headers
                    .get("x-ratelimit-remaining")
                    .map(String::as_str)
                    != Some("0") =>
            {
                Err(ProviderError::Authentication {
                    connection_id: self.config.connection_id.clone(),
                    message: github_message(&response.body),
                })
            }
            403 | 429 => Err(ProviderError::RateLimited {
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
            409 | 412 | 422 => Err(ProviderError::Conflict {
                ticket: url.into(),
                message: github_message(&response.body),
            }),
            _ => Err(ProviderError::Conflict {
                ticket: url.into(),
                message: format!(
                    "GitHub returned {}: {}",
                    response.status,
                    github_message(&response.body)
                ),
            }),
        }
    }

    fn json<T: for<'de> Deserialize<'de>>(
        &self,
        response: HttpResponse,
    ) -> Result<T, ProviderError> {
        serde_json::from_str(&response.body).map_err(|error| ProviderError::Conflict {
            ticket: self.config.connection_id.clone(),
            message: format!("invalid GitHub response: {error}"),
        })
    }

    fn issue(&self, native_id: &str) -> Result<GitHubIssue, ProviderError> {
        validate_number(native_id)?;
        let response = self.request("GET", &self.endpoint(&format!("issues/{native_id}")), None)?;
        self.json(response)
    }

    fn comments(&self, native_id: &str) -> Result<Vec<GitHubComment>, ProviderError> {
        let response = self.request(
            "GET",
            &self.endpoint(&format!("issues/{native_id}/comments?per_page=100")),
            None,
        )?;
        self.json(response)
    }

    fn api_ticket(&self, issue: GitHubIssue, comments: Vec<GitHubComment>) -> ApiTicket {
        let labels = issue
            .labels
            .iter()
            .map(|label| label.name.clone())
            .collect::<Vec<_>>();
        let category = mapped_label(&labels, "category:").unwrap_or_else(|| "issue".into());
        let priority = mapped_label(&labels, "priority:")
            .as_deref()
            .and_then(parse_priority)
            .unwrap_or_default();
        let status = if issue.state == "closed" {
            Status::Completed
        } else if labels.iter().any(|label| label == "status:started") {
            Status::Started
        } else if labels.iter().any(|label| label == "status:backlog") {
            Status::Backlog
        } else {
            Status::NotStarted
        };
        let close_reason = if issue.state == "closed" {
            Some(match issue.state_reason.as_deref() {
                Some("not_planned") => CloseReason::NotPlanned,
                _ => CloseReason::Completed,
            })
        } else {
            None
        };
        let native_id = issue.number.to_string();
        ApiTicket {
            connection_id: self.config.connection_id.clone(),
            native_id: native_id.clone(),
            qualified_id: format!("{}:{native_id}", self.config.connection_id),
            native_url: Some(issue.html_url),
            concurrency_token: Some(issue.updated_at.clone()),
            id: native_id.clone(),
            slug: format!("{}#{native_id}", self.config.repository),
            title: issue.title,
            details: strip_transfer_markers(issue.body.clone().unwrap_or_default()),
            category,
            priority,
            status,
            up_next: false,
            tags: labels
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
            close_reason,
            duplicate_of: None,
            copied_from: None,
            transfer_operation_id: transfer_marker(&issue.body, "operation"),
            transferred_from: transfer_marker(&issue.body, "source"),
            moved_to_store: None,
            moved_at: None,
            claimed_by: None,
            claim_lease_expires_at: None,
            worker_label: None,
            claim_count: 0,
            assignees: issue.assignees.into_iter().map(|user| user.login).collect(),
            review_requests: vec![],
            schema: 1,
            notes: comments
                .into_iter()
                .map(|comment| ApiNote {
                    id: comment.id.to_string(),
                    kind: NoteKind::Regular,
                    created_at: comment.created_at.clone(),
                    edited_at: comment.updated_at.unwrap_or(comment.created_at),
                    summary: None,
                    text: strip_note_marker(comment.body),
                })
                .collect(),
            attachments: vec![],
            auto_context: vec![],
        }
    }

    fn list_issues(&self, updated_after: Option<&str>) -> Result<Vec<GitHubIssue>, ProviderError> {
        let mut url = self.endpoint("issues?state=all&per_page=100");
        if let Some(since) = updated_after {
            url.push_str("&since=");
            url.push_str(since);
        }
        let mut issues = Vec::new();
        loop {
            let response = self.request("GET", &url, None)?;
            let next = response
                .headers
                .get("link")
                .and_then(|link| next_link(link));
            let page: Vec<GitHubIssue> = self.json(response)?;
            issues.extend(
                page.into_iter()
                    .filter(|issue| issue.pull_request.is_none()),
            );
            let Some(next) = next else { break };
            url = next;
        }
        Ok(issues)
    }
}

impl TicketProvider for GitHubProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            connection_id: self.config.connection_id.clone(),
            provider: "github".into(),
            display_name: format!("GitHub {}", self.config.repository),
            locator: self.config.repository.clone(),
            default: self.config.default,
            capabilities: github_capabilities(),
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
            return Err(ProviderError::Unsupported {
                connection_id: self.config.connection_id.clone(),
                capability: "requested query filter",
            });
        }
        let mut tickets =
            self.list_issues(query.updated_after.as_deref())?
                .into_iter()
                .map(|issue| self.api_ticket(issue, vec![]))
                .filter(|ticket| query.status.is_none_or(|status| ticket.status == status))
                .filter(|ticket| {
                    query
                        .priority
                        .is_none_or(|priority| ticket.priority == priority)
                })
                .filter(|ticket| query.tags.iter().all(|tag| ticket.tags.contains(tag)))
                .filter(|ticket| {
                    query
                        .category
                        .as_deref()
                        .is_none_or(|category| ticket.category == category)
                })
                .filter(|ticket| {
                    if query.open_only {
                        ticket.close_reason.is_none()
                    } else {
                        true
                    }
                })
                .filter(|ticket| {
                    query
                        .close_reason
                        .is_none_or(|reason| ticket.close_reason == Some(reason))
                })
                .filter(|ticket| {
                    query
                        .closed
                        .is_none_or(|closed| ticket.close_reason.is_some() == closed)
                })
                .filter(|ticket| {
                    query.assignee.as_deref().is_none_or(|assignee| {
                        ticket.assignees.iter().any(|value| value == assignee)
                    })
                })
                .filter(|ticket| {
                    query
                        .created_after
                        .as_deref()
                        .is_none_or(|after| ticket.created_at.as_str() >= after)
                        && query
                            .created_before
                            .as_deref()
                            .is_none_or(|before| ticket.created_at.as_str() <= before)
                        && query
                            .updated_after
                            .as_deref()
                            .is_none_or(|after| ticket.updated_at.as_str() >= after)
                        && query
                            .updated_before
                            .as_deref()
                            .is_none_or(|before| ticket.updated_at.as_str() <= before)
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
                transfer_marker(&issue.body, "operation").as_deref() == Some(operation_id)
            })
            .map(|issue| self.api_ticket(issue, vec![])))
    }

    fn get(&self, native_id: &str) -> Result<ApiTicket, ProviderError> {
        Ok(self.api_ticket(self.issue(native_id)?, self.comments(native_id)?))
    }

    fn create(
        &self,
        _ctx: MutationContext,
        draft: ProviderDraft,
    ) -> Result<ApiTicket, ProviderError> {
        if !draft.blocked_by.is_empty() {
            return Err(ProviderError::Unsupported {
                connection_id: self.config.connection_id.clone(),
                capability: "dependencies",
            });
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
        let mut body = draft.details;
        if let Some(transfer) = draft.transfer {
            body.push_str(&format!(
                "\n\n<!-- hotsheet-transfer {} -->",
                json!({"operation":transfer.operation_id,"source":transfer.source.qualified()})
            ));
        }
        let labels = mapped_labels(
            &draft.category,
            draft.priority,
            &draft.tags,
            Some(draft.status),
        );
        let response = self.request(
            "POST",
            &self.endpoint("issues"),
            Some(&json!({"title":draft.title,"body":body,"labels":labels})),
        )?;
        let issue: GitHubIssue = self.json(response)?;
        Ok(self.api_ticket(issue, vec![]))
    }

    fn update(
        &self,
        native_id: &str,
        _now: Timestamp,
        patch: ProviderPatch,
    ) -> Result<ApiTicket, ProviderError> {
        validate_number(native_id)?;
        if patch.blocked_reason.is_some() {
            return self.unsupported("blocked_reason");
        }
        if patch.blocked_by.as_ref().is_some_and(|v| !v.is_empty()) {
            return Err(ProviderError::Unsupported {
                connection_id: self.config.connection_id.clone(),
                capability: "dependencies",
            });
        }
        let current = self.issue(native_id)?;
        if patch
            .expected_token
            .as_deref()
            .is_some_and(|token| token != current.updated_at)
        {
            return Err(ProviderError::Conflict {
                ticket: format!("{}:{native_id}", self.config.connection_id),
                message: "issue changed since it was read".into(),
            });
        }
        let current_ticket = self.api_ticket(current.clone(), vec![]);
        let category = patch.category.unwrap_or(current_ticket.category);
        let priority = patch.priority.unwrap_or(current_ticket.priority);
        let tags = patch.tags.unwrap_or(current_ticket.tags);
        let status = patch.status.unwrap_or(current_ticket.status);
        let state = if matches!(
            status,
            Status::Completed | Status::Verified | Status::Archive | Status::Deleted
        ) {
            "closed"
        } else {
            "open"
        };
        let labels = mapped_labels(&category, priority, &tags, Some(status));
        let mut body = patch
            .details
            .unwrap_or_else(|| current.body.clone().unwrap_or_default());
        if let Some(marker) = transfer_suffix(&current.body)
            && !body.contains("<!-- hotsheet-transfer ")
        {
            body.push_str(&marker);
        }
        let response = self.request(
            "PATCH",
            &self.endpoint(&format!("issues/{native_id}")),
            Some(&json!({
                "title": patch.title.unwrap_or(current.title),
                "body": body,
                "state": state,
                "labels": labels,
            })),
        )?;
        let issue: GitHubIssue = self.json(response)?;
        Ok(self.api_ticket(issue, vec![]))
    }

    fn add_note(
        &self,
        native_id: &str,
        ctx: MutationContext,
        _kind: NoteKind,
        text: String,
    ) -> Result<ApiTicket, ProviderError> {
        let marker = format!("<!-- hotsheet-note-id:{} -->", ctx.generated_id);
        if self
            .comments(native_id)?
            .iter()
            .any(|comment| comment.body.contains(&marker))
        {
            return self.get(native_id);
        }
        self.request(
            "POST",
            &self.endpoint(&format!("issues/{native_id}/comments")),
            Some(&json!({"body":format!("{text}\n\n{marker}")})),
        )?;
        self.get(native_id)
    }

    fn close(
        &self,
        native_id: &str,
        _now: Timestamp,
        reason: CloseReason,
        _duplicate_of: Option<String>,
    ) -> Result<ApiTicket, ProviderError> {
        let state_reason = match reason {
            CloseReason::NotPlanned | CloseReason::Obsolete | CloseReason::Duplicate => {
                "not_planned"
            }
            CloseReason::Completed => "completed",
        };
        let response = self.request(
            "PATCH",
            &self.endpoint(&format!("issues/{native_id}")),
            Some(&json!({"state":"closed","state_reason":state_reason})),
        )?;
        let issue: GitHubIssue = self.json(response)?;
        Ok(self.api_ticket(issue, vec![]))
    }

    fn assign(
        &self,
        native_id: &str,
        _now: Timestamp,
        assignees: Option<Vec<String>>,
        reviews: Vec<ReviewRequest>,
    ) -> Result<ApiTicket, ProviderError> {
        if !reviews.is_empty() {
            return Err(ProviderError::Unsupported {
                connection_id: self.config.connection_id.clone(),
                capability: "review_requests",
            });
        }
        let response = self.request(
            "PATCH",
            &self.endpoint(&format!("issues/{native_id}")),
            Some(&json!({"assignees":assignees.unwrap_or_default()})),
        )?;
        let issue: GitHubIssue = self.json(response)?;
        Ok(self.api_ticket(issue, vec![]))
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

impl GitHubProvider {
    fn unsupported<T>(&self, capability: &'static str) -> Result<T, ProviderError> {
        Err(ProviderError::Unsupported {
            connection_id: self.config.connection_id.clone(),
            capability,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct GitHubIssue {
    number: u64,
    title: String,
    body: Option<String>,
    state: String,
    state_reason: Option<String>,
    html_url: String,
    created_at: String,
    updated_at: String,
    closed_at: Option<String>,
    #[serde(default)]
    labels: Vec<GitHubLabel>,
    #[serde(default)]
    assignees: Vec<GitHubUser>,
    pull_request: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct GitHubLabel {
    name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct GitHubUser {
    login: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubComment {
    id: u64,
    body: String,
    created_at: String,
    #[serde(default)]
    updated_at: Option<String>,
}

fn github_capabilities() -> ProviderCapabilities {
    ProviderCapabilities {
        create: true,
        update: true,
        close: true,
        notes: true,
        note_edit: false,
        note_delete: false,
        attachments: false,
        assignment: true,
        review_requests: false,
        dependencies: false,
        up_next: false,
        close_reasons: true,
        claims: false,
        atomic_batch: false,
        not_working_report: false,
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

fn validate_number(native_id: &str) -> Result<(), ProviderError> {
    native_id
        .parse::<u64>()
        .map(|_| ())
        .map_err(|_| ProviderError::InvalidNativeId(native_id.into()))
}

fn parse_priority(value: &str) -> Option<Priority> {
    match value {
        "lowest" => Some(Priority::Lowest),
        "low" => Some(Priority::Low),
        "default" | "normal" | "medium" => Some(Priority::Default),
        "high" => Some(Priority::High),
        "highest" | "critical" | "urgent" => Some(Priority::Highest),
        _ => None,
    }
}

fn priority_name(priority: Priority) -> &'static str {
    match priority {
        Priority::Lowest => "lowest",
        Priority::Low => "low",
        Priority::Default => "default",
        Priority::High => "high",
        Priority::Highest => "highest",
    }
}

fn priority_rank(priority: Priority) -> u8 {
    match priority {
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
    if let Some(Status::Started) = status {
        labels.push("status:started".into());
    }
    if let Some(Status::Backlog) = status {
        labels.push("status:backlog".into());
    }
    labels.sort();
    labels.dedup();
    labels
}

fn github_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| value.get("message")?.as_str().map(str::to_string))
        .unwrap_or_else(|| body.to_string())
}

fn next_link(header: &str) -> Option<String> {
    header.split(',').find_map(|part| {
        let (url, relation) = part.trim().split_once(';')?;
        (relation.trim() == "rel=\"next\"").then(|| {
            url.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string()
        })
    })
}

fn transfer_marker(body: &Option<String>, field: &str) -> Option<String> {
    let marker = body
        .as_deref()?
        .split("<!-- hotsheet-transfer ")
        .nth(1)?
        .split(" -->")
        .next()?;
    serde_json::from_str::<Value>(marker)
        .ok()?
        .get(field)?
        .as_str()
        .map(str::to_string)
}

fn transfer_suffix(body: &Option<String>) -> Option<String> {
    let marker = body
        .as_deref()?
        .split("\n\n<!-- hotsheet-transfer ")
        .nth(1)?;
    Some(format!("\n\n<!-- hotsheet-transfer {marker}"))
}

fn strip_transfer_markers(body: String) -> String {
    body.split("\n\n<!-- hotsheet-transfer ")
        .next()
        .unwrap_or(&body)
        .to_string()
}

fn strip_note_marker(body: String) -> String {
    body.split("\n\n<!-- hotsheet-note-id:")
        .next()
        .unwrap_or(&body)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    type RecordedRequest = (String, String, Vec<(String, String)>, Option<Value>);

    #[derive(Default)]
    struct FakeTransport {
        responses: Mutex<VecDeque<HttpResponse>>,
        requests: Mutex<Vec<RecordedRequest>>,
    }

    impl FakeTransport {
        fn with(responses: Vec<HttpResponse>) -> Arc<Self> {
            Arc::new(Self {
                responses: Mutex::new(responses.into()),
                requests: Mutex::new(vec![]),
            })
        }
    }

    impl GitHubTransport for FakeTransport {
        fn request(
            &self,
            method: &str,
            url: &str,
            headers: &[(&str, String)],
            body: Option<&Value>,
        ) -> Result<HttpResponse, String> {
            self.requests.lock().unwrap().push((
                method.into(),
                url.into(),
                headers
                    .iter()
                    .map(|(name, value)| ((*name).into(), value.clone()))
                    .collect(),
                body.cloned(),
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

    fn issue(number: u64, title: &str, body: &str) -> Value {
        json!({
            "number": number,
            "title": title,
            "body": body,
            "state": "open",
            "state_reason": null,
            "html_url": format!("https://github.com/acme/widgets/issues/{number}"),
            "created_at": "2026-08-26T00:00:00Z",
            "updated_at": "2026-08-26T00:01:00Z",
            "closed_at": null,
            "labels": [{"name":"category:bug"},{"name":"priority:high"},{"name":"customer"}],
            "assignees": [{"login":"octocat"}],
            "pull_request": null
        })
    }

    fn provider(transport: Arc<dyn GitHubTransport>) -> GitHubProvider {
        let mut config = GitHubConfig::new("github-main", "acme/widgets", "test-token");
        config.api_base = "https://api.test".into();
        GitHubProvider::new(config, transport)
    }

    #[test]
    fn maps_native_issue_comments_labels_identity_and_authorization() {
        let transport = FakeTransport::with(vec![
            response(200, issue(42, "broken widget", "details")),
            response(
                200,
                json!([{"id":91,"body":"investigating\n\n<!-- hotsheet-note-id:x -->","created_at":"2026-08-26T00:02:00Z"}]),
            ),
        ]);
        let ticket = provider(transport.clone()).get("42").unwrap();
        assert_eq!(ticket.qualified_id, "github-main:42");
        assert_eq!(
            ticket.native_url.as_deref(),
            Some("https://github.com/acme/widgets/issues/42")
        );
        assert_eq!(ticket.category, "bug");
        assert_eq!(ticket.priority, Priority::High);
        assert_eq!(ticket.tags, ["customer"]);
        assert_eq!(ticket.assignees, ["octocat"]);
        assert_eq!(ticket.notes[0].text, "investigating");
        let requests = transport.requests.lock().unwrap();
        assert!(
            requests[0]
                .2
                .iter()
                .any(|(name, value)| name == "Authorization" && value == "Bearer test-token")
        );
    }

    #[test]
    fn paginates_filters_pull_requests_and_surfaces_rate_limits() {
        let mut first = response(
            200,
            json!([issue(1, "one", "") , {
                "number": 2, "title":"pr", "body":"", "state":"open", "state_reason":null,
                "html_url":"https://github.com/acme/widgets/pull/2", "created_at":"2026-08-26T00:00:00Z",
                "updated_at":"2026-08-26T00:00:00Z", "closed_at":null, "labels":[], "assignees":[],
                "pull_request": {"url":"https://api.test/pr/2"}
            }]),
        );
        first.headers.insert(
            "link".into(),
            "<https://api.test/page2>; rel=\"next\"".into(),
        );
        let mut limited = response(403, json!({"message":"rate limit exceeded"}));
        limited
            .headers
            .insert("x-ratelimit-remaining".into(), "0".into());
        limited.headers.insert("retry-after".into(), "60".into());
        let transport = FakeTransport::with(vec![
            first,
            response(200, json!([issue(3, "three", "")])),
            limited,
        ]);
        let provider = provider(transport);
        let tickets = provider.query(&TicketQuery::default()).unwrap();
        assert_eq!(
            tickets
                .iter()
                .map(|ticket| ticket.native_id.as_str())
                .collect::<Vec<_>>(),
            ["1", "3"]
        );
        assert!(matches!(
            provider.get("9"),
            Err(ProviderError::RateLimited {
                retry_after_seconds: Some(60),
                ..
            })
        ));
    }

    #[test]
    fn transfer_create_records_native_marker_and_retry_resolves_existing() {
        let marker_body = r#"body

<!-- hotsheet-transfer {"operation":"op-1","source":"git:ABC"} -->"#;
        let transport = FakeTransport::with(vec![
            response(200, json!([])),
            response(201, issue(7, "copied", marker_body)),
            response(200, json!([issue(7, "copied", marker_body)])),
        ]);
        let provider = provider(transport.clone());
        let draft = ProviderDraft {
            title: "copied".into(),
            category: "task".into(),
            priority: Priority::Default,
            status: hotsheet_model::Status::NotStarted,
            details: "body".into(),
            tags: vec![],
            up_next: false,
            blocked_by: vec![],
            transfer: Some(hotsheet_ticketing::TransferProvenance {
                operation_id: "op-1".into(),
                source: hotsheet_ticketing::TicketRef {
                    connection_id: "git".into(),
                    native_id: "ABC".into(),
                },
            }),
        };
        let created = provider
            .create(
                MutationContext {
                    now: Timestamp::new("2026-08-26T00:00:00Z"),
                    generated_id: hotsheet_model::Ulid::new(),
                },
                draft.clone(),
            )
            .unwrap();
        let retry = provider
            .create(
                MutationContext {
                    now: Timestamp::new("2026-08-26T00:01:00Z"),
                    generated_id: hotsheet_model::Ulid::new(),
                },
                draft,
            )
            .unwrap();
        assert_eq!(created.native_id, retry.native_id);
        assert_eq!(created.transferred_from.as_deref(), Some("git:ABC"));
        assert_eq!(
            transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| request.0 == "POST")
                .count(),
            1
        );
    }

    #[test]
    fn stale_concurrency_token_conflicts_before_remote_update() {
        let transport =
            FakeTransport::with(vec![response(200, issue(42, "changed elsewhere", "body"))]);
        let error = provider(transport.clone())
            .update(
                "42",
                Timestamp::new("2026-08-26T01:00:00Z"),
                ProviderPatch {
                    expected_token: Some("older-token".into()),
                    title: Some("my update".into()),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(error, ProviderError::Conflict { .. }));
        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn blocked_reason_updates_are_explicitly_unsupported() {
        let transport = FakeTransport::with(vec![]);
        let provider = provider(transport.clone());
        for blocked_reason in [Some(Some("waiting".into())), Some(None)] {
            assert!(matches!(
                provider.update(
                    "42",
                    Timestamp::new("2026-08-26T01:00:00Z"),
                    ProviderPatch {
                        blocked_reason,
                        ..Default::default()
                    },
                ),
                Err(ProviderError::Unsupported {
                    capability: "blocked_reason",
                    ..
                })
            ));
        }
        assert!(transport.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn webhook_is_an_invalidation_reference_not_a_mirrored_ticket() {
        let event = parse_webhook(
            "issue_comment",
            br#"{"action":"created","issue":{"number":77},"comment":{"body":"remote"}}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(event.native_id, "77");
        assert_eq!(event.action, "created");
        assert!(parse_webhook("push", br#"{}"#).unwrap().is_none());
    }

    #[test]
    #[ignore = "creates and closes a real GitHub issue; set HOTSHEET_GITHUB_LIVE_REPO and store github-live in the OS keychain (or set HOTSHEET_GITHUB_LIVE_TOKEN)"]
    fn github_live_crud_against_dedicated_test_repository() {
        let repository = std::env::var("HOTSHEET_GITHUB_LIVE_REPO").expect("live repository");
        let token = std::env::var("HOTSHEET_GITHUB_LIVE_TOKEN").unwrap_or_else(|_| {
            hotsheet_ticketing::KeyRegistry::new("", hotsheet_ticketing::OsKeychain)
                .get("github-live")
                .expect("HOTSHEET_GITHUB_LIVE_TOKEN or OS-keychain entry 'github-live'")
        });
        let mut config = GitHubConfig::new("github-live", repository, token);
        if let Ok(base) = std::env::var("HOTSHEET_GITHUB_LIVE_API_BASE") {
            config.api_base = base;
        }
        let provider = GitHubProvider::live(config);
        let created = provider
            .create(
                MutationContext {
                    now: Timestamp::new("2026-08-26T00:00:00Z"),
                    generated_id: hotsheet_model::Ulid::new(),
                },
                ProviderDraft {
                    title: format!("Hot Sheet provider live test {}", hotsheet_model::Ulid::new()),
                    category: "test".into(),
                    priority: Priority::Default,
                    status: hotsheet_model::Status::NotStarted,
                    details: "Created by an opt-in Hot Sheet provider validation; this issue will be closed automatically.".into(),
                    tags: vec![],
                    up_next: false,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        let read = provider.get(&created.native_id).unwrap();
        assert_eq!(read.native_id, created.native_id);
        provider
            .add_note(
                &created.native_id,
                MutationContext {
                    now: Timestamp::new("2026-08-26T00:01:00Z"),
                    generated_id: hotsheet_model::Ulid::new(),
                },
                NoteKind::Regular,
                "Hot Sheet live comment validation".into(),
            )
            .unwrap();
        let closed = provider
            .close(
                &created.native_id,
                Timestamp::new("2026-08-26T00:02:00Z"),
                CloseReason::Completed,
                None,
            )
            .unwrap();
        assert_eq!(closed.close_reason, Some(CloseReason::Completed));
    }
}
