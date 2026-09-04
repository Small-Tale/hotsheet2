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
pub struct JiraConfig {
    pub connection_id: String,
    pub project_key: String,
    pub base_url: String,
    pub email: String,
    pub token: String,
    pub default: bool,
}

impl JiraConfig {
    pub fn from_connection(
        connection: &ProviderConnection,
        token: impl Into<String>,
    ) -> Result<Self, ProviderError> {
        let base_url = connection.settings.get("base_url").and_then(Value::as_str);
        let email = connection.settings.get("email").and_then(Value::as_str);
        if connection.provider != "jira"
            || connection.locator.is_empty()
            || base_url.is_none()
            || email.is_none()
        {
            return Err(ProviderError::Conflict {
                ticket: connection.id.clone(),
                message:
                    "Jira requires locator=<project key>, settings.base_url, and settings.email"
                        .into(),
            });
        }
        Ok(Self {
            connection_id: connection.id.clone(),
            project_key: connection.locator.clone(),
            base_url: base_url.unwrap().trim_end_matches('/').into(),
            email: email.unwrap().into(),
            token: token.into(),
            default: connection.default,
        })
    }
}

#[derive(Clone)]
pub struct JiraProvider {
    config: JiraConfig,
    transport: Arc<dyn HttpTransport>,
}

impl JiraProvider {
    pub fn new(config: JiraConfig, transport: Arc<dyn HttpTransport>) -> Self {
        Self { config, transport }
    }

    pub fn live(config: JiraConfig) -> Self {
        Self::new(config, Arc::new(UreqGitHubTransport))
    }

    fn endpoint(&self, suffix: &str) -> String {
        format!(
            "{}/rest/api/3/{}",
            self.config.base_url,
            suffix.trim_start_matches('/')
        )
    }

    fn request(
        &self,
        method: &str,
        url: &str,
        body: Option<&Value>,
    ) -> Result<HttpResponse, ProviderError> {
        let credentials = base64(&format!("{}:{}", self.config.email, self.config.token));
        let headers = [
            ("Authorization", format!("Basic {credentials}")),
            ("Accept", "application/json".into()),
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
            401 | 403 => Err(ProviderError::Authentication {
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
            message: format!("invalid Jira response: {error}"),
        })
    }

    fn issue(&self, key: &str) -> Result<JiraIssue, ProviderError> {
        validate_key(key)?;
        let response = self.request("GET", &self.endpoint(&format!("issue/{key}")), None)?;
        self.json(response)
    }

    fn comments(&self, key: &str) -> Result<Vec<JiraComment>, ProviderError> {
        let response = self.request(
            "GET",
            &self.endpoint(&format!("issue/{key}/comment?maxResults=100")),
            None,
        )?;
        Ok(self.json::<JiraComments>(response)?.comments)
    }

    fn list_issues(&self, updated_after: Option<&str>) -> Result<Vec<JiraIssue>, ProviderError> {
        let mut next_page_token: Option<String> = None;
        let mut issues = Vec::new();
        loop {
            let mut jql = format!("project = {} ORDER BY updated ASC", self.config.project_key);
            if let Some(after) = updated_after {
                jql = format!(
                    "project = {} AND updated >= \"{}\" ORDER BY updated ASC",
                    self.config.project_key, after
                );
            }
            let response = self.request(
                "POST",
                &self.endpoint("search/jql"),
                Some(&json!({
                    "jql":jql,"nextPageToken":next_page_token,"maxResults":100,
                    "fields":["summary","description","status","priority","issuetype","labels","assignee","created","updated","resolutiondate"]
                })),
            )?;
            let page: JiraSearch = self.json(response)?;
            issues.extend(page.issues);
            if page.is_last || page.next_page_token.is_none() {
                break;
            }
            next_page_token = page.next_page_token;
        }
        Ok(issues)
    }

    fn ticket(&self, issue: JiraIssue, comments: Vec<JiraComment>) -> ApiTicket {
        let details_with_marker = adf_to_text(issue.fields.description.as_ref());
        let status = match issue.fields.status.status_category.key.as_str() {
            "done" => Status::Completed,
            "indeterminate" => Status::Started,
            _ => Status::NotStarted,
        };
        let priority = issue
            .fields
            .priority
            .as_ref()
            .and_then(|value| parse_priority(&value.name))
            .unwrap_or_default();
        let url = format!("{}/browse/{}", self.config.base_url, issue.key);
        ApiTicket {
            connection_id: self.config.connection_id.clone(),
            native_id: issue.key.clone(),
            qualified_id: format!("{}:{}", self.config.connection_id, issue.key),
            native_url: Some(url),
            concurrency_token: Some(issue.fields.updated.clone()),
            id: issue.key.clone(),
            slug: issue.key,
            title: issue.fields.summary,
            details: strip_transfer(&details_with_marker),
            category: issue.fields.issue_type.name,
            priority,
            status,
            up_next: false,
            tags: issue.fields.labels,
            blocked_by: vec![],
            blocked_reason: None,
            created_at: issue.fields.created,
            updated_at: issue.fields.updated,
            completed_at: issue.fields.resolution_date.clone(),
            verified_at: None,
            closed_at: issue.fields.resolution_date,
            close_reason: (status == Status::Completed).then_some(CloseReason::Completed),
            duplicate_of: None,
            copied_from: None,
            transfer_operation_id: marker(&details_with_marker, "operation"),
            transferred_from: marker(&details_with_marker, "source"),
            moved_to_store: None,
            moved_at: None,
            claimed_by: None,
            claim_lease_expires_at: None,
            worker_label: None,
            claim_count: 0,
            assignees: issue
                .fields
                .assignee
                .into_iter()
                .map(|user| user.account_id)
                .collect(),
            review_requests: vec![],
            schema: 1,
            notes: comments
                .into_iter()
                .map(|comment| ApiNote {
                    id: comment.id,
                    kind: NoteKind::Regular,
                    created_at: comment.created.clone(),
                    edited_at: comment.updated.unwrap_or(comment.created),
                    summary: None,
                    text: strip_note(&adf_to_text(Some(&comment.body))),
                })
                .collect(),
            attachments: vec![],
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

impl TicketProvider for JiraProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            connection_id: self.config.connection_id.clone(),
            provider: "jira".into(),
            display_name: format!("Jira {}", self.config.project_key),
            locator: self.config.project_key.clone(),
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
            || query.close_reason.is_some()
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
                marker(&adf_to_text(issue.fields.description.as_ref()), "operation").as_deref()
                    == Some(operation_id)
            })
            .map(|issue| self.ticket(issue, vec![])))
    }

    fn get(&self, native_id: &str) -> Result<ApiTicket, ProviderError> {
        Ok(self.ticket(self.issue(native_id)?, self.comments(native_id)?))
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
        let mut details = draft.details;
        if let Some(transfer) = draft.transfer {
            details.push_str(&format!(
                "\n\n<!-- hotsheet-transfer {} -->",
                json!({"operation":transfer.operation_id,"source":transfer.source.qualified()})
            ));
        }
        let response = self.request(
            "POST",
            &self.endpoint("issue"),
            Some(&json!({"fields":{
                "project":{"key":self.config.project_key},"summary":draft.title,
                "description":text_to_adf(&details),"issuetype":{"name":draft.category},
                "priority":{"name":priority_name(draft.priority)},"labels":draft.tags
            }})),
        )?;
        let created: JiraCreated = self.json(response)?;
        self.get(&created.key)
    }

    fn update(
        &self,
        native_id: &str,
        _: Timestamp,
        patch: ProviderPatch,
    ) -> Result<ApiTicket, ProviderError> {
        if patch.blocked_reason.is_some() {
            return self.unsupported("blocked_reason");
        }
        let current = self.issue(native_id)?;
        if patch
            .expected_token
            .as_deref()
            .is_some_and(|value| value != current.fields.updated)
        {
            return Err(ProviderError::Conflict {
                ticket: format!("{}:{native_id}", self.config.connection_id),
                message: "issue changed since it was read".into(),
            });
        }
        if patch.status.is_some() {
            return self.unsupported("status transitions");
        }
        if patch
            .blocked_by
            .as_ref()
            .is_some_and(|value| !value.is_empty())
        {
            return self.unsupported("dependencies");
        }
        let current_details = adf_to_text(current.fields.description.as_ref());
        let mut details = patch.details.unwrap_or_else(|| current_details.clone());
        if let Some(suffix) = transfer_suffix(&current_details)
            && !details.contains("<!-- hotsheet-transfer ")
        {
            details.push_str(&suffix);
        }
        let fields = json!({
            "summary":patch.title.unwrap_or(current.fields.summary),
            "description":text_to_adf(&details),
            "issuetype":{"name":patch.category.unwrap_or(current.fields.issue_type.name)},
            "priority":{"name":priority_name(patch.priority.unwrap_or_else(||current.fields.priority.as_ref().and_then(|value|parse_priority(&value.name)).unwrap_or_default()))},
            "labels":patch.tags.unwrap_or(current.fields.labels)
        });
        self.request(
            "PUT",
            &self.endpoint(&format!("issue/{native_id}")),
            Some(&json!({"fields":fields})),
        )?;
        self.get(native_id)
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
            .comments(native_id)?
            .iter()
            .any(|comment| adf_to_text(Some(&comment.body)).contains(&note_marker))
        {
            return self.get(native_id);
        }
        self.request(
            "POST",
            &self.endpoint(&format!("issue/{native_id}/comment")),
            Some(&json!({"body":text_to_adf(&format!("{text}\n\n{note_marker}"))})),
        )?;
        self.get(native_id)
    }

    fn close(
        &self,
        _: &str,
        _: Timestamp,
        _: CloseReason,
        _: Option<String>,
    ) -> Result<ApiTicket, ProviderError> {
        self.unsupported("close transitions require project workflow mapping")
    }

    fn assign(
        &self,
        native_id: &str,
        _: Timestamp,
        assignees: Option<Vec<String>>,
        reviews: Vec<ReviewRequest>,
    ) -> Result<ApiTicket, ProviderError> {
        if !reviews.is_empty() {
            return self.unsupported("review_requests");
        }
        let assignees = assignees.unwrap_or_default();
        if assignees.len() > 1 {
            return self.unsupported("multiple assignees");
        }
        self.request("PUT",&self.endpoint(&format!("issue/{native_id}")),Some(&json!({"fields":{"assignee":assignees.first().map(|id|json!({"accountId":id})).unwrap_or(Value::Null)}})))?;
        self.get(native_id)
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
struct JiraIssue {
    key: String,
    fields: JiraFields,
}
#[derive(Debug, Clone, Deserialize)]
struct JiraFields {
    summary: String,
    description: Option<Value>,
    status: JiraStatus,
    priority: Option<JiraNamed>,
    #[serde(rename = "issuetype")]
    issue_type: JiraNamed,
    #[serde(default)]
    labels: Vec<String>,
    assignee: Option<JiraUser>,
    created: String,
    updated: String,
    #[serde(rename = "resolutiondate")]
    resolution_date: Option<String>,
}
#[derive(Debug, Clone, Deserialize)]
struct JiraStatus {
    #[serde(rename = "statusCategory")]
    status_category: JiraStatusCategory,
}
#[derive(Debug, Clone, Deserialize)]
struct JiraStatusCategory {
    key: String,
}
#[derive(Debug, Clone, Deserialize)]
struct JiraNamed {
    name: String,
}
#[derive(Debug, Clone, Deserialize)]
struct JiraUser {
    #[serde(rename = "accountId")]
    account_id: String,
}
#[derive(Debug, Deserialize)]
struct JiraSearch {
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
    #[serde(rename = "isLast", default)]
    is_last: bool,
    issues: Vec<JiraIssue>,
}
#[derive(Debug, Deserialize)]
struct JiraComments {
    comments: Vec<JiraComment>,
}
#[derive(Debug, Clone, Deserialize)]
struct JiraComment {
    id: String,
    body: Value,
    created: String,
    #[serde(default)]
    updated: Option<String>,
}
#[derive(Debug, Deserialize)]
struct JiraCreated {
    key: String,
}

fn capabilities() -> ProviderCapabilities {
    ProviderCapabilities {
        create: true,
        update: true,
        close: false,
        notes: true,
        note_edit: false,
        note_delete: false,
        attachments: false,
        assignment: true,
        review_requests: false,
        dependencies: false,
        up_next: false,
        close_reasons: false,
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
            "closed",
            "created_at",
            "updated_at",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
    }
}
fn validate_key(value: &str) -> Result<(), ProviderError> {
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Err(ProviderError::InvalidNativeId(value.into()))
    } else {
        Ok(())
    }
}
fn response_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("errorMessages")?
                .as_array()?
                .first()?
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.into())
}
fn parse_priority(value: &str) -> Option<Priority> {
    match value.to_ascii_lowercase().as_str() {
        "lowest" => Some(Priority::Lowest),
        "low" => Some(Priority::Low),
        "medium" => Some(Priority::Default),
        "high" => Some(Priority::High),
        "highest" => Some(Priority::Highest),
        _ => None,
    }
}
fn priority_name(value: Priority) -> &'static str {
    match value {
        Priority::Lowest => "Lowest",
        Priority::Low => "Low",
        Priority::Default => "Medium",
        Priority::High => "High",
        Priority::Highest => "Highest",
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
fn text_to_adf(text: &str) -> Value {
    json!({"type":"doc","version":1,"content":text.split('\n').map(|line|json!({"type":"paragraph","content":if line.is_empty(){vec![]}else{vec![json!({"type":"text","text":line})]}})).collect::<Vec<_>>()})
}
fn adf_to_text(value: Option<&Value>) -> String {
    fn collect(value: &Value, out: &mut Vec<String>) {
        if value.get("type").and_then(Value::as_str) == Some("text")
            && let Some(text) = value.get("text").and_then(Value::as_str)
        {
            out.push(text.into())
        }
        if value.get("type").and_then(Value::as_str) == Some("paragraph") && !out.is_empty() {
            out.push("\n".into())
        }
        if let Some(content) = value.get("content").and_then(Value::as_array) {
            for child in content {
                collect(child, out)
            }
        }
    }
    let mut out = Vec::new();
    if let Some(value) = value {
        collect(value, &mut out)
    }
    out.concat().trim_end_matches('\n').into()
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
fn transfer_suffix(body: &str) -> Option<String> {
    Some(format!(
        "\n\n<!-- hotsheet-transfer {}",
        body.split("\n\n<!-- hotsheet-transfer ").nth(1)?
    ))
}
fn strip_note(body: &str) -> String {
    body.split("\n\n<!-- hotsheet-note-id:")
        .next()
        .unwrap_or(body)
        .into()
}
fn base64(value: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = value.as_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = ((chunk[0] as u32) << 16)
            | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8)
            | (chunk.get(2).copied().unwrap_or(0) as u32);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;

    type RecordedRequest = (String, String, Vec<(String, String)>, Option<Value>);

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
            body: Option<&Value>,
        ) -> Result<HttpResponse, String> {
            self.requests.lock().unwrap().push((
                method.into(),
                url.into(),
                headers
                    .iter()
                    .map(|(k, v)| ((*k).into(), v.clone()))
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
    fn issue(key: &str, title: &str) -> Value {
        json!({"key":key,"fields":{"summary":title,"description":text_to_adf("line one\nline two"),"status":{"statusCategory":{"key":"indeterminate"}},"priority":{"name":"High"},"issuetype":{"name":"Bug"},"labels":["customer"],"assignee":{"accountId":"acct-1"},"created":"2026-08-26T00:00:00Z","updated":"2026-08-26T00:01:00Z","resolutiondate":null}})
    }
    fn provider(fake: Arc<Fake>) -> JiraProvider {
        JiraProvider::new(
            JiraConfig {
                connection_id: "jira-eng".into(),
                project_key: "ENG".into(),
                base_url: "https://jira.test".into(),
                email: "dev@example.com".into(),
                token: "token".into(),
                default: false,
            },
            fake,
        )
    }

    #[test]
    fn maps_adf_identity_and_basic_auth() {
        let fake=Arc::new(Fake{responses:Mutex::new(vec![response(200,issue("ENG-42","broken")),response(200,json!({"comments":[{"id":"7","body":text_to_adf("comment"),"created":"2026-08-26T00:02:00Z"}]}))].into()),..Default::default()});
        let ticket = provider(fake.clone()).get("ENG-42").unwrap();
        assert_eq!(ticket.qualified_id, "jira-eng:ENG-42");
        assert_eq!(ticket.details, "line one\nline two");
        assert_eq!(ticket.status, Status::Started);
        assert_eq!(ticket.priority, Priority::High);
        assert_eq!(ticket.assignees, ["acct-1"]);
        assert_eq!(ticket.notes[0].text, "comment");
        assert!(
            fake.requests.lock().unwrap()[0]
                .2
                .iter()
                .any(|(k, v)| k == "Authorization" && v.starts_with("Basic "))
        );
    }

    #[test]
    fn jira_search_paginates_and_uses_incremental_jql() {
        let fake=Arc::new(Fake{responses:Mutex::new(vec![response(200,json!({"nextPageToken":"page-2","isLast":false,"issues":[issue("ENG-1","one")]})),response(200,json!({"isLast":true,"issues":[issue("ENG-2","two")]}))].into()),..Default::default()});
        let query = TicketQuery {
            updated_after: Some("2026-08-01".into()),
            ..Default::default()
        };
        let tickets = provider(fake.clone()).query(&query).unwrap();
        assert_eq!(tickets.len(), 2);
        let requests = fake.requests.lock().unwrap();
        assert!(
            requests[0].3.as_ref().unwrap()["jql"]
                .as_str()
                .unwrap()
                .contains("updated >=")
        );
        assert_eq!(requests[1].3.as_ref().unwrap()["nextPageToken"], "page-2");
    }

    #[test]
    fn jira_applies_date_bounds_after_incremental_fetch() {
        let mut later = issue("ENG-2", "two");
        later["fields"]["created"] = json!("2026-08-27T00:00:00Z");
        later["fields"]["updated"] = json!("2026-08-27T00:01:00Z");
        let fake = Arc::new(Fake {
            responses: Mutex::new(
                vec![response(
                    200,
                    json!({"isLast":true,"issues":[issue("ENG-1", "one"),later]}),
                )]
                .into(),
            ),
            ..Default::default()
        });
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
            ["ENG-1"]
        );
    }

    #[test]
    fn jira_capabilities_reject_workflow_specific_close_and_stale_updates() {
        let fake = Arc::new(Fake {
            responses: Mutex::new(vec![response(200, issue("ENG-9", "remote"))].into()),
            ..Default::default()
        });
        let provider = provider(fake);
        assert!(!provider.descriptor().capabilities.close);
        assert!(matches!(
            provider.close("ENG-9", Timestamp::new("x"), CloseReason::Completed, None),
            Err(ProviderError::Unsupported { .. })
        ));
        assert!(matches!(
            provider.update(
                "ENG-9",
                Timestamp::new("x"),
                ProviderPatch {
                    expected_token: Some("stale".into()),
                    ..Default::default()
                }
            ),
            Err(ProviderError::Conflict { .. })
        ));
    }

    #[test]
    fn blocked_reason_updates_are_explicitly_unsupported() {
        let fake = Arc::new(Fake::default());
        let provider = provider(fake.clone());
        for blocked_reason in [Some(Some("waiting".into())), Some(None)] {
            assert!(matches!(
                provider.update(
                    "ENG-9",
                    Timestamp::new("x"),
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
        assert!(fake.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn jira_rate_limit_is_typed() {
        let mut limited = response(429, json!({"errorMessages":["slow down"]}));
        limited.headers.insert("retry-after".into(), "30".into());
        let fake = Arc::new(Fake {
            responses: Mutex::new(vec![limited].into()),
            ..Default::default()
        });
        assert!(matches!(
            provider(fake).get("ENG-1"),
            Err(ProviderError::RateLimited {
                retry_after_seconds: Some(30),
                ..
            })
        ));
    }

    #[test]
    #[ignore = "reads a real Jira project; set HOTSHEET_JIRA_LIVE_BASE_URL/PROJECT/EMAIL/TOKEN"]
    fn jira_live_contract_drift() {
        let connection = ProviderConnection {
            id: "jira-live".into(),
            provider: "jira".into(),
            locator: std::env::var("HOTSHEET_JIRA_LIVE_PROJECT").expect("live project"),
            name: None,
            default: false,
            settings: json!({
                "base_url":std::env::var("HOTSHEET_JIRA_LIVE_BASE_URL").expect("base url"),
                "email":std::env::var("HOTSHEET_JIRA_LIVE_EMAIL").expect("email")
            }),
        };
        let provider = JiraProvider::live(
            JiraConfig::from_connection(
                &connection,
                std::env::var("HOTSHEET_JIRA_LIVE_TOKEN").expect("live token"),
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
