use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use hotsheet_model::{CloseReason, NoteKind, Priority, ReviewRequest, Status, Timestamp};
use hotsheet_ticketing::ops::NoteMetadataInput;
use hotsheet_ticketing::{
    ApiNote, ApiTicket, MutationContext, ProviderCapabilities, ProviderConnection,
    ProviderDescriptor, ProviderDraft, ProviderError, ProviderKeysetPage, ProviderPatch,
    ProviderTicketPage, ProviderTicketSummary, SortKey, TicketProvider, TicketQuery,
    checkout_order::MergeKey, compare_provider_tickets, filter_provider_ticket_page,
    keyset_page_from_native_pages, keyset_page_from_rows, provider_text_matches, unbounded_query,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::github_attachments::{self, AttachmentMarker, GitHubAttachmentRepository};
use crate::note_trailer;
use hotsheet_ticketing::wire::ApiAttachment;

/// Native page size for value-keyset walks; fixed so resume hints stay positionally valid.
const NATIVE_KEYSET_PAGE: usize = 100;
/// Coalesce the summary and status-scoped board reads issued together by one project refresh.
const ISSUE_LIST_CACHE_TTL: Duration = Duration::from_secs(10);

const API_VERSION: &str = "2022-11-28";

#[derive(Debug, Clone)]
pub struct GitHubConfig {
    pub connection_id: String,
    pub repository: String,
    pub api_base: String,
    pub token: String,
    pub default: bool,
    /// Assets repository for attachments (HS2-HSA64D); attachments are unsupported without one.
    pub attachments: Option<GitHubAttachmentRepository>,
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
            attachments: None,
        }
    }

    /// Enable attachments through an assets repository.
    pub fn with_attachments(mut self, attachments: GitHubAttachmentRepository) -> Self {
        self.attachments = Some(attachments);
        self
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
            attachments: GitHubAttachmentRepository::from_settings(&connection.settings).map_err(
                |message| ProviderError::Conflict {
                    ticket: connection.id.clone(),
                    message,
                },
            )?,
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
        let mut request = ureq::request(method, url).timeout(std::time::Duration::from_secs(30));
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
    issues_cache: Option<IssueListCache>,
    rate_limited_until: Option<Arc<Mutex<Option<Instant>>>>,
}

type IssueListCache = Arc<Mutex<Option<(Instant, Vec<GitHubIssue>)>>>;

impl GitHubProvider {
    pub fn new(config: GitHubConfig, transport: Arc<dyn GitHubTransport>) -> Self {
        Self {
            config,
            transport,
            issues_cache: None,
            rate_limited_until: None,
        }
    }

    pub fn live(config: GitHubConfig) -> Self {
        let mut provider = Self::new(config, Arc::new(UreqGitHubTransport));
        provider.issues_cache = Some(Arc::new(Mutex::new(None)));
        provider.rate_limited_until = Some(Arc::new(Mutex::new(None)));
        provider
    }

    fn endpoint(&self, suffix: &str) -> String {
        format!(
            "{}/repos/{}/{}",
            self.config.api_base.trim_end_matches('/'),
            self.config.repository,
            suffix.trim_start_matches('/')
        )
    }

    fn repository_endpoint(&self, repository: &str, suffix: &str) -> String {
        format!(
            "{}/repos/{repository}/{}",
            self.config.api_base.trim_end_matches('/'),
            suffix.trim_start_matches('/')
        )
    }

    fn request(
        &self,
        method: &str,
        url: &str,
        body: Option<&Value>,
    ) -> Result<HttpResponse, ProviderError> {
        if let Some(cooldown) = &self.rate_limited_until {
            if let Ok(mut until) = cooldown.lock() {
                if let Some(remaining) =
                    until.and_then(|deadline| deadline.checked_duration_since(Instant::now()))
                {
                    return Err(ProviderError::RateLimited {
                        connection_id: self.config.connection_id.clone(),
                        retry_after_seconds: Some(remaining.as_secs().max(1)),
                    });
                }
                *until = None;
            }
        }
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
        let rate_limited = response.status == 429
            || (response.status == 403
                && (response
                    .headers
                    .get("x-ratelimit-remaining")
                    .map(String::as_str)
                    == Some("0")
                    || response.headers.contains_key("retry-after")
                    || github_message(&response.body)
                        .to_ascii_lowercase()
                        .contains("rate limit")));
        match response.status {
            200..=299 => {
                if method != "GET" {
                    if let Some(cache) = &self.issues_cache {
                        if let Ok(mut cached) = cache.lock() {
                            *cached = None;
                        }
                    }
                }
                Ok(response)
            }
            401 | 403 if !rate_limited => {
                let message = github_message(&response.body);
                let message = if response.headers.contains_key("x-github-sso") {
                    format!(
                        "{message}; authorize Hot Sheet for your SAML organization and try again"
                    )
                } else if response.status == 401 {
                    format!("{message}; sign in with GitHub again")
                } else {
                    message
                };
                Err(ProviderError::Authentication {
                    connection_id: self.config.connection_id.clone(),
                    message,
                })
            }
            403 | 429 if rate_limited => {
                let seconds = response
                    .headers
                    .get("retry-after")
                    .and_then(|value| value.parse::<u64>().ok())
                    .or_else(|| {
                        response
                            .headers
                            .get("x-ratelimit-reset")
                            .and_then(|value| value.parse::<u64>().ok())
                            .map(|reset| {
                                reset
                                    .saturating_sub(
                                        SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                    )
                                    .max(1)
                            })
                    })
                    .unwrap_or(60);
                if let Some(cooldown) = &self.rate_limited_until {
                    if let Ok(mut until) = cooldown.lock() {
                        *until = Some(Instant::now() + Duration::from_secs(seconds));
                    }
                }
                Err(ProviderError::RateLimited {
                    connection_id: self.config.connection_id.clone(),
                    retry_after_seconds: Some(seconds),
                })
            }
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

    /// Every issue comment, oldest first (HS2-9GS5TS). Comments page in ascending order, so a
    /// single request would drop the newest ones past the first 100 — including the latest
    /// scored note and an `add_note` idempotency marker. A short page ends the list.
    fn comments(&self, native_id: &str) -> Result<Vec<GitHubComment>, ProviderError> {
        const PAGE: usize = 100;
        let mut comments = Vec::new();
        for page in 1..=MAX_HISTORY_PAGES {
            let response = self.request(
                "GET",
                &self.endpoint(&format!(
                    "issues/{native_id}/comments?per_page={PAGE}&page={page}"
                )),
                None,
            )?;
            let batch: Vec<GitHubComment> = self.json(response)?;
            let len = batch.len();
            comments.extend(batch);
            if len < PAGE {
                break;
            }
        }
        Ok(comments)
    }

    /// Every `reopened` issue event, oldest first (HS2-N3RMTV). Issue events page in
    /// ascending order, so read every page; a short page ends the history.
    fn reopen_times(&self, native_id: &str) -> Result<Vec<String>, ProviderError> {
        const PAGE: usize = 100;
        let mut reopens = Vec::new();
        for page in 1..=MAX_HISTORY_PAGES {
            let response = self.request(
                "GET",
                &self.endpoint(&format!(
                    "issues/{native_id}/events?per_page={PAGE}&page={page}"
                )),
                None,
            )?;
            let events: Vec<GitHubIssueEvent> = self.json(response)?;
            let len = events.len();
            reopens.extend(
                events
                    .into_iter()
                    .filter(|event| event.event == "reopened")
                    .map(|event| event.created_at),
            );
            if len < PAGE {
                break;
            }
        }
        Ok(reopens)
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
        let status = issue_status(&issue.state, &labels);
        // Attachment link comments this provider wrote project as attachments, not notes.
        let mut attachments = Vec::new();
        let notes = comments
            .into_iter()
            .filter_map(|comment| {
                if let Some((id, marker)) = github_attachments::parse_comment(&comment.body) {
                    attachments.push(github_attachments::api_attachment(
                        id,
                        marker,
                        comment.created_at,
                    ));
                    return None;
                }
                Some(comment)
            })
            .map(|comment| {
                let (text, confidence) = note_trailer::parse_comment(&comment.body);
                ApiNote {
                    id: comment.id.to_string(),
                    kind: NoteKind::Regular,
                    created_at: comment.created_at.clone(),
                    edited_at: comment.updated_at.unwrap_or(comment.created_at),
                    summary: None,
                    confidence,
                    actor: None,
                    text,
                }
            })
            .collect::<Vec<_>>();
        let latest_confidence = note_trailer::latest_confidence(status, &notes, None);
        let close_reason = if issue.state == "closed" {
            Some(
                close_reason_from_labels(&labels).unwrap_or(match issue.state_reason.as_deref() {
                    Some("not_planned") => CloseReason::NotPlanned,
                    _ => CloseReason::Completed,
                }),
            )
        } else {
            None
        };
        let duplicate_of = if issue.state == "closed" {
            mapped_label(&labels, DUPLICATE_OF_PREFIX)
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
            started_phase: None,
            up_next: status.is_active() && labels.iter().any(|label| label == "up-next"),
            feedback_needed: false,
            tags: labels
                .into_iter()
                .filter(|label| !is_provider_label(label))
                .collect(),
            blocked_by: vec![],
            blocked_reason: None,
            created_at: issue.created_at,
            updated_at: issue.updated_at,
            completed_at: issue.closed_at.clone(),
            verified_at: (status == Status::Verified)
                .then(|| issue.closed_at.clone())
                .flatten(),
            closed_at: issue.closed_at,
            close_reason,
            duplicate_of,
            copied_from: None,
            transfer_operation_id: transfer_marker(&issue.body, "operation"),
            transferred_from: transfer_marker(&issue.body, "source"),
            moved_to_store: None,
            moved_at: None,
            claimed_by: None,
            claim_lease_expires_at: None,
            claim_eta_at: None,
            claim_started_at: None,
            worker_label: None,
            legacy_number: None,
            claim_count: 0,
            claim_history: vec![],
            assignees: issue.assignees.into_iter().map(|user| user.login).collect(),
            review_requests: vec![],
            schema: 1,
            notes,
            latest_confidence,
            attachments,
            warnings: vec![],
            auto_context: vec![],
        }
    }

    /// Reject filters the native page API cannot evaluate (shared by paged reads).
    fn check_query_filters(&self, query: &TicketQuery) -> Result<(), ProviderError> {
        if query.review_requested.is_some()
            || query.review_by.is_some()
            || query.claimed.is_some()
            || query.blocked.is_some()
            || query.page_after.is_some()
            || query.completed_after.is_some()
            || query.min_confidence.is_some()
            || query.max_confidence.is_some()
            || query.completed_before.is_some()
            || query.verified_after.is_some()
            || query.verified_before.is_some()
            || query.has_attachment.is_some()
            || query.has_media_annotation.is_some()
            || !query.attachment_patterns.is_empty()
        {
            return Err(ProviderError::Unsupported {
                connection_id: self.config.connection_id.clone(),
                capability: "requested query filter",
            });
        }
        Ok(())
    }

    fn issue_page(
        &self,
        cursor: Option<&str>,
        updated_after: Option<&str>,
        limit: usize,
    ) -> Result<(Vec<GitHubIssue>, Option<String>), ProviderError> {
        let mut url = if let Some(cursor) = cursor {
            let api_prefix = format!("{}/", self.config.api_base.trim_end_matches('/'));
            if !cursor.starts_with(&api_prefix) || !cursor.contains("/issues?") {
                return Err(ProviderError::Conflict {
                    ticket: self.config.connection_id.clone(),
                    message: "invalid GitHub page cursor".into(),
                });
            }
            cursor.to_owned()
        } else {
            self.endpoint(&format!(
                "issues?state=all&direction=asc&per_page={}",
                limit.min(100)
            ))
        };
        if let Some(since) = updated_after {
            if cursor.is_none() {
                url.push_str("&since=");
                url.push_str(since);
            }
        }
        let response = self.request("GET", &url, None)?;
        let next = response
            .headers
            .get("link")
            .and_then(|link| next_link(link));
        let issues = self
            .json::<Vec<GitHubIssue>>(response)?
            .into_iter()
            .filter(|issue| issue.pull_request.is_none())
            .collect();
        Ok((issues, next))
    }

    fn list_issues(&self, updated_after: Option<&str>) -> Result<Vec<GitHubIssue>, ProviderError> {
        if updated_after.is_none() {
            if let Some(cache) = &self.issues_cache {
                if let Ok(mut cached) = cache.lock() {
                    if let Some((at, issues)) = cached.as_ref() {
                        if at.elapsed() < ISSUE_LIST_CACHE_TTL {
                            return Ok(issues.clone());
                        }
                    }
                    // Keep this per-provider lock through the refresh: parallel board columns
                    // share one GitHub walk instead of all consuming the remaining API budget.
                    let issues = self.fetch_issues(None)?;
                    *cached = Some((Instant::now(), issues.clone()));
                    return Ok(issues);
                }
            }
        }
        self.fetch_issues(updated_after)
    }

    fn fetch_issues(&self, updated_after: Option<&str>) -> Result<Vec<GitHubIssue>, ProviderError> {
        let mut cursor = None;
        let mut issues = Vec::new();
        loop {
            let (page, next) = self.issue_page(cursor.as_deref(), updated_after, 100)?;
            issues.extend(page);
            let Some(next) = next else { break };
            cursor = Some(next);
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
            capabilities: github_capabilities(self.config.attachments.is_some()),
        }
    }

    fn query(&self, query: &TicketQuery) -> Result<Vec<ApiTicket>, ProviderError> {
        if query.review_requested.is_some()
            || query.review_by.is_some()
            || query.claimed.is_some()
            || query.blocked.is_some()
            || query.page_after.is_some()
            || query.completed_after.is_some()
            || query.min_confidence.is_some()
            || query.max_confidence.is_some()
            || query.completed_before.is_some()
            || query.verified_after.is_some()
            || query.verified_before.is_some()
            || query.has_attachment.is_some()
            || query.has_media_annotation.is_some()
            || !query.attachment_patterns.is_empty()
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
                .filter(|ticket| provider_text_matches(ticket, query.text.as_deref()))
                .filter(|ticket| query.status.is_none_or(|status| ticket.status == status))
                .filter(|ticket| !query.up_next_only || ticket.up_next)
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
        tickets.sort_by(|left, right| {
            compare_provider_tickets(left, right, query.sort, query.descending)
        });
        if let Some(limit) = query.limit {
            tickets.truncate(limit);
        }
        Ok(tickets)
    }

    fn query_page(
        &self,
        query: &TicketQuery,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ProviderTicketPage, ProviderError> {
        self.check_query_filters(query)?;
        if query.descending || !matches!(query.sort, SortKey::Id | SortKey::Created) {
            let mut unbounded = query.clone();
            unbounded.limit = None;
            unbounded.page_after = None;
            let rows = self.query(&unbounded)?;
            let offset =
                cursor
                    .unwrap_or("0")
                    .parse::<usize>()
                    .map_err(|_| ProviderError::Conflict {
                        ticket: self.config.connection_id.clone(),
                        message: "invalid provider page cursor".into(),
                    })?;
            let end = offset.saturating_add(limit).min(rows.len());
            return Ok(ProviderTicketPage {
                items: rows[offset.min(rows.len())..end].to_vec(),
                next_cursor: (end < rows.len()).then(|| end.to_string()),
            });
        }
        let (issues, next_cursor) =
            self.issue_page(cursor, query.updated_after.as_deref(), limit)?;
        Ok(ProviderTicketPage {
            items: filter_provider_ticket_page(
                issues
                    .into_iter()
                    .map(|issue| self.api_ticket(issue, vec![]))
                    .collect(),
                query,
            ),
            next_cursor,
        })
    }

    /// Value keyset (HS2-74H84S). Only ascending creation order is natively keyset-safe:
    /// GitHub orders issue numbers numerically while the checkout `id` order compares
    /// native ids as strings, so every other sort resumes over the full filtered result.
    fn query_after(
        &self,
        query: &TicketQuery,
        after: Option<&MergeKey>,
        resume: Option<&str>,
        limit: usize,
    ) -> Result<ProviderKeysetPage, ProviderError> {
        if query.descending || query.sort != SortKey::Created {
            return Ok(keyset_page_from_rows(
                self.query(&unbounded_query(query))?,
                query,
                after,
                limit,
            ));
        }
        self.check_query_filters(query)?;
        keyset_page_from_native_pages(query, after, resume, limit, |cursor| {
            let (issues, next) =
                self.issue_page(cursor, query.updated_after.as_deref(), NATIVE_KEYSET_PAGE)?;
            Ok((
                issues
                    .into_iter()
                    .map(|issue| self.api_ticket(issue, vec![]))
                    .collect(),
                next,
            ))
        })
    }

    fn summary(
        &self,
        now: &str,
        day_starts: &[String],
    ) -> Result<ProviderTicketSummary, ProviderError> {
        let mut summary = ProviderTicketSummary::default();
        for issue in self.list_issues(None)? {
            summary.add_ticket(&self.api_ticket(issue, vec![]), now, day_starts);
        }
        Ok(summary)
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
        let mut ticket = self.api_ticket(self.issue(native_id)?, self.comments(native_id)?);
        if note_trailer::needs_reopen_history(ticket.status, &ticket.notes) {
            let reopens = self.reopen_times(native_id)?;
            ticket.latest_confidence = note_trailer::latest_confidence(
                ticket.status,
                &ticket.notes,
                note_trailer::last_reopen(reopens.iter().map(String::as_str)),
            );
        }
        Ok(ticket)
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
            draft.up_next,
            None,
            None,
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
        if patch.started_phase.is_some() {
            return self.unsupported("started_phase");
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
        let up_next = patch.up_next.unwrap_or(current_ticket.up_next);
        // A closed issue keeps the outcome it was closed with (HS2-K8R3T8): GitHub resets
        // `state_reason` to `completed` on any PATCH that omits it, so every closed write
        // re-sends the current reason and its `closed:` / `duplicate-of:` labels. Reopening
        // drops them so a later close starts clean.
        let closed = closes_issue(status);
        let close_reason = closed.then(|| {
            current_ticket
                .close_reason
                .unwrap_or(CloseReason::Completed)
        });
        let duplicate_of = if closed {
            current_ticket.duplicate_of.clone()
        } else {
            None
        };
        let labels = mapped_labels(
            &category,
            priority,
            &tags,
            Some(status),
            up_next,
            close_reason,
            duplicate_of.as_deref(),
        );
        let mut body = patch
            .details
            .unwrap_or_else(|| current.body.clone().unwrap_or_default());
        if let Some(marker) = transfer_suffix(&current.body)
            && !body.contains("<!-- hotsheet-transfer ")
        {
            body.push_str(&marker);
        }
        let mut payload = json!({
            "title": patch.title.unwrap_or(current.title),
            "body": body,
            "state": if closed { "closed" } else { "open" },
            "labels": labels,
        });
        if let Some(reason) = close_reason {
            payload["state_reason"] = json!(github_state_reason(reason));
        }
        let response = self.request(
            "PATCH",
            &self.endpoint(&format!("issues/{native_id}")),
            Some(&payload),
        )?;
        let issue: GitHubIssue = self.json(response)?;
        Ok(self.api_ticket(issue, vec![]))
    }

    fn add_note(
        &self,
        native_id: &str,
        ctx: MutationContext,
        kind: NoteKind,
        text: String,
    ) -> Result<ApiTicket, ProviderError> {
        self.add_note_with_metadata(native_id, ctx, kind, NoteMetadataInput::default(), text)
    }

    /// The confidence rides on a `Confidence: NN%` comment trailer (HS2-5YNASC).
    fn add_note_with_metadata(
        &self,
        native_id: &str,
        ctx: MutationContext,
        _kind: NoteKind,
        metadata: NoteMetadataInput,
        text: String,
    ) -> Result<ApiTicket, ProviderError> {
        let marker = note_trailer::note_marker(ctx.generated_id);
        if self
            .comments(native_id)?
            .iter()
            .any(|comment| comment.body.contains(&marker))
        {
            return self.get(native_id);
        }
        let body = note_trailer::compose_comment(&text, metadata.confidence, ctx.generated_id);
        self.request(
            "POST",
            &self.endpoint(&format!("issues/{native_id}/comments")),
            Some(&json!({ "body": body })),
        )?;
        self.get(native_id)
    }

    /// Upload to the assets repository, then link it from one marked issue comment
    /// (HS2-HSA64D). The marker makes a retry return the existing attachment.
    fn add_attachment(
        &self,
        native_id: &str,
        attachment: ApiAttachment,
        bytes: Vec<u8>,
    ) -> Result<ApiTicket, ProviderError> {
        validate_number(native_id)?;
        let Some(assets) = self.config.attachments.clone() else {
            return self.unsupported("attachments");
        };
        if !github_attachments::valid_attachment_id(&attachment.id) {
            return Err(ProviderError::InvalidNativeId {
                provider: "github",
                id: attachment.id,
            });
        }
        let key = github_attachments::marker_key(&attachment.id);
        if self
            .comments(native_id)?
            .iter()
            .any(|comment| comment.body.contains(&key))
        {
            return self.get(native_id);
        }
        let path = assets.file_path(&attachment.id, &attachment.filename);
        let file = self.upload_asset(&assets, &path, &attachment.filename, native_id, &bytes)?;
        let url = assets.link_url(&self.config.api_base, &file.path, file.html_url.as_deref());
        let marker = AttachmentMarker {
            filename: attachment.filename,
            path: file.path,
            repository: assets.repository,
            branch: assets.branch,
            sha: Some(file.sha),
            batch_id: attachment.batch_id,
            batch_label: attachment.batch_label,
            actor: attachment.actor,
            purpose: attachment.purpose,
        };
        let body = github_attachments::compose_comment(&attachment.id, &url, &marker);
        self.request(
            "POST",
            &self.endpoint(&format!("issues/{native_id}/comments")),
            Some(&json!({ "body": body })),
        )?;
        self.get(native_id)
    }

    /// Read an attachment's bytes through the authenticated API, so private assets
    /// repositories render without exposing a token to the browser.
    fn attachment_bytes(
        &self,
        native_id: &str,
        attachment_id: &str,
    ) -> Result<Vec<u8>, ProviderError> {
        validate_number(native_id)?;
        let marker = self
            .comments(native_id)?
            .into_iter()
            .find_map(|comment| {
                github_attachments::parse_comment(&comment.body)
                    .filter(|(id, _)| id == attachment_id)
                    .map(|(_, marker)| marker)
            })
            .ok_or_else(|| ProviderError::NotFound {
                connection_id: self.config.connection_id.clone(),
                native_id: attachment_id.into(),
            })?;
        let encoded = match &marker.sha {
            Some(sha) => {
                let blob: GitHubBlob = self.json(self.request(
                    "GET",
                    &self.repository_endpoint(&marker.repository, &format!("git/blobs/{sha}")),
                    None,
                )?)?;
                blob.content
            }
            None => {
                let file: GitHubContentFile = self.json(self.request(
                    "GET",
                    &self.repository_endpoint(
                        &marker.repository,
                        &format!(
                            "contents/{}?ref={}",
                            github_attachments::encode_path(&marker.path),
                            github_attachments::encode_path(&marker.branch)
                        ),
                    ),
                    None,
                )?)?;
                file.content.unwrap_or_default()
            }
        };
        let compact: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();
        BASE64
            .decode(compact)
            .map_err(|error| ProviderError::Conflict {
                ticket: format!("{}:{native_id}", self.config.connection_id),
                message: format!("invalid attachment content from GitHub: {error}"),
            })
    }

    fn close(
        &self,
        native_id: &str,
        _now: Timestamp,
        reason: CloseReason,
        duplicate_of: Option<String>,
    ) -> Result<ApiTicket, ProviderError> {
        // GitHub only records `completed` / `not_planned`; the exact Hot Sheet reason and the
        // duplicate target ride on provider-owned labels so they survive a round trip
        // (HS2-K8R3T8). The current labels are read first so ordinary tags are kept.
        let current = self.api_ticket(self.issue(native_id)?, vec![]);
        let labels = mapped_labels(
            &current.category,
            current.priority,
            &current.tags,
            Some(Status::Completed),
            false,
            Some(reason),
            duplicate_of.as_deref(),
        );
        let response = self.request(
            "PATCH",
            &self.endpoint(&format!("issues/{native_id}")),
            Some(&json!({
                "state": "closed",
                "state_reason": github_state_reason(reason),
                "labels": labels,
            })),
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
    /// Commit one attachment file to the assets repository. A path that already exists
    /// (a retry after the upload succeeded but the link comment failed) is reused.
    fn upload_asset(
        &self,
        assets: &GitHubAttachmentRepository,
        path: &str,
        filename: &str,
        native_id: &str,
        bytes: &[u8],
    ) -> Result<GitHubContentFile, ProviderError> {
        let url = self.repository_endpoint(
            &assets.repository,
            &format!("contents/{}", github_attachments::encode_path(path)),
        );
        let body = json!({
            "message": format!(
                "Upload attachment: {filename} ({}#{native_id})",
                self.config.repository
            ),
            "content": BASE64.encode(bytes),
            "branch": assets.branch,
        });
        match self.request("PUT", &url, Some(&body)) {
            Ok(response) => {
                let written: GitHubContentWrite = self.json(response)?;
                Ok(written.content)
            }
            Err(error @ ProviderError::Conflict { .. }) => {
                let existing = self.request(
                    "GET",
                    &format!(
                        "{url}?ref={}",
                        github_attachments::encode_path(&assets.branch)
                    ),
                    None,
                );
                match existing {
                    Ok(response) => self.json(response),
                    Err(_) => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    }

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
struct GitHubContentWrite {
    content: GitHubContentFile,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubContentFile {
    path: String,
    sha: String,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubBlob {
    content: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubIssueEvent {
    event: String,
    created_at: String,
}

/// Upper bound on history pages read for one detail request (100 entries each).
const MAX_HISTORY_PAGES: usize = 50;

#[derive(Debug, Clone, Deserialize)]
struct GitHubComment {
    id: u64,
    body: String,
    created_at: String,
    #[serde(default)]
    updated_at: Option<String>,
}

fn github_capabilities(attachments: bool) -> ProviderCapabilities {
    ProviderCapabilities {
        create: true,
        update: true,
        close: true,
        notes: true,
        note_edit: false,
        note_delete: false,
        // Append-only evidence through the configured assets repository (HS2-HSA64D).
        attachments,
        attachment_edit: false,
        assignment: true,
        review_requests: false,
        dependencies: false,
        up_next: true,
        close_reasons: true,
        claims: false,
        atomic_batch: false,
        not_working_report: false,
        // Comment trailers for scores are HS2-5YNASC; until then a score fails explicitly.
        note_confidence: true,
        offline_mutation: false,
        history: true,
        watch: true,
        provider_idempotency: false,
        query_fields: [
            "status",
            "priority",
            "up_next",
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
        .map_err(|_| ProviderError::InvalidNativeId {
            provider: "github",
            id: native_id.into(),
        })
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

fn mapped_label(labels: &[String], prefix: &str) -> Option<String> {
    labels
        .iter()
        .find_map(|label| label.strip_prefix(prefix).map(str::to_string))
}

/// Label prefix carrying the canonical ticket of a duplicate close (`duplicate-of:<ref>`).
const DUPLICATE_OF_PREFIX: &str = "duplicate-of:";

/// Prefixes of the labels the provider owns. They carry Hot Sheet state GitHub has no field
/// for and are never surfaced as ordinary tags.
const PROVIDER_LABEL_PREFIXES: [&str; 5] = [
    "category:",
    "priority:",
    "status:",
    "closed:",
    DUPLICATE_OF_PREFIX,
];

fn is_provider_label(label: &str) -> bool {
    label == "up-next"
        || PROVIDER_LABEL_PREFIXES
            .iter()
            .any(|prefix| label.starts_with(prefix))
}

/// Whether a Hot Sheet status is represented by a closed GitHub issue.
fn closes_issue(status: Status) -> bool {
    matches!(
        status,
        Status::Completed | Status::Verified | Status::Archive | Status::Deleted
    )
}

/// The `status:` label for states GitHub's open/closed pair cannot express on its own.
/// Started and Backlog qualify an open issue; Verified and Archived qualify a closed one.
fn status_label(status: Status) -> Option<&'static str> {
    match status {
        Status::Started => Some("status:started"),
        Status::Backlog => Some("status:backlog"),
        Status::Verified => Some("status:verified"),
        Status::Archive => Some("status:archived"),
        Status::NotStarted | Status::Completed | Status::Deleted | Status::Moved => None,
    }
}

/// The `closed:` label that keeps the exact Hot Sheet close reason beside GitHub's coarser
/// `not_planned`. Completed and Not planned are GitHub's own reasons and need no label.
fn close_reason_label(reason: CloseReason) -> Option<&'static str> {
    match reason {
        CloseReason::Completed | CloseReason::NotPlanned => None,
        CloseReason::Duplicate => Some("closed:duplicate"),
        CloseReason::Obsolete => Some("closed:obsolete"),
        CloseReason::WorksAsDesigned => Some("closed:works-as-designed"),
    }
}

fn close_reason_from_labels(labels: &[String]) -> Option<CloseReason> {
    labels.iter().find_map(|label| match label.as_str() {
        "closed:duplicate" => Some(CloseReason::Duplicate),
        "closed:obsolete" => Some(CloseReason::Obsolete),
        "closed:works-as-designed" => Some(CloseReason::WorksAsDesigned),
        _ => None,
    })
}

/// The Hot Sheet status of an issue: `state` decides open/closed and a `status:` label
/// refines it. A stale open-only label on a closed issue (or vice versa) is ignored.
fn issue_status(state: &str, labels: &[String]) -> Status {
    let has = |name: &str| labels.iter().any(|label| label == name);
    if state == "closed" {
        if has("status:verified") {
            Status::Verified
        } else if has("status:archived") {
            Status::Archive
        } else {
            Status::Completed
        }
    } else if has("status:started") {
        Status::Started
    } else if has("status:backlog") {
        Status::Backlog
    } else {
        Status::NotStarted
    }
}

fn mapped_labels(
    category: &str,
    priority: Priority,
    tags: &[String],
    status: Option<Status>,
    up_next: bool,
    close_reason: Option<CloseReason>,
    duplicate_of: Option<&str>,
) -> Vec<String> {
    let mut labels = tags
        .iter()
        .filter(|tag| !is_provider_label(tag))
        .cloned()
        .collect::<Vec<_>>();
    labels.push(format!("category:{category}"));
    labels.push(format!("priority:{}", priority_name(priority)));
    if let Some(label) = status.and_then(status_label) {
        labels.push(label.into());
    }
    if up_next && status.is_some_and(Status::is_active) {
        labels.push("up-next".into());
    }
    if let Some(label) = close_reason.and_then(close_reason_label) {
        labels.push(label.into());
    }
    if let Some(target) = duplicate_of.filter(|target| !target.is_empty()) {
        labels.push(format!("{DUPLICATE_OF_PREFIX}{target}"));
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

pub(crate) fn next_link(header: &str) -> Option<String> {
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

/// GitHub records only `completed` or `not_planned`; every Hot Sheet reason that means
/// "no change was made" (not planned, duplicate, obsolete, works as designed) closes as
/// `not_planned`.
fn github_state_reason(reason: CloseReason) -> &'static str {
    match reason {
        CloseReason::Completed => "completed",
        CloseReason::NotPlanned
        | CloseReason::Obsolete
        | CloseReason::Duplicate
        | CloseReason::WorksAsDesigned => "not_planned",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_numeric_id_is_rejected_as_a_github_id_shape_mismatch() {
        // HS2-GKERTK: the error names the provider that rejected the id, so a git ULID probed
        // against a linked GitHub source is not reported as a git-provider failure.
        let error = validate_number("01M3NAHJV0JM0QR0FHWYW6H1B2").unwrap_err();
        assert!(matches!(
            error,
            ProviderError::InvalidNativeId {
                provider: "github",
                ..
            }
        ));
        assert_eq!(
            error.to_string(),
            "invalid native id '01M3NAHJV0JM0QR0FHWYW6H1B2' for the github provider"
        );
        assert!(validate_number("42").is_ok());
    }

    #[test]
    fn close_reasons_map_to_githubs_two_state_reasons() {
        assert_eq!(github_state_reason(CloseReason::Completed), "completed");
        for reason in [
            CloseReason::NotPlanned,
            CloseReason::Duplicate,
            CloseReason::Obsolete,
            CloseReason::WorksAsDesigned,
        ] {
            assert_eq!(github_state_reason(reason), "not_planned", "{reason:?}");
        }
    }
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
    fn up_next_label_survives_edits_and_clears_on_close() {
        let open = issue(42, "widget", "details");
        let mut queued = open.clone();
        queued["labels"] = json!([
            {"name":"category:bug"}, {"name":"priority:high"},
            {"name":"customer"}, {"name":"up-next"}
        ]);
        let mut closed = queued.clone();
        closed["state"] = json!("closed");
        closed["closed_at"] = json!("2026-08-27T00:00:00Z");
        closed["labels"] = json!([
            {"name":"category:bug"}, {"name":"priority:high"}, {"name":"customer"}
        ]);
        let transport = FakeTransport::with(vec![
            response(200, open.clone()),
            response(200, queued.clone()),
            response(200, queued.clone()),
            response(200, queued.clone()),
            response(200, queued.clone()),
            response(200, open.clone()),
            response(200, open.clone()),
            response(200, queued.clone()),
            response(200, queued.clone()),
            response(200, closed),
        ]);
        let github = provider(transport.clone());
        let now = Timestamp::new("2026-08-28T00:00:00Z");
        assert!(
            github
                .update(
                    "42",
                    now.clone(),
                    ProviderPatch {
                        up_next: Some(true),
                        ..Default::default()
                    }
                )
                .unwrap()
                .up_next
        );
        assert!(
            github
                .update(
                    "42",
                    now.clone(),
                    ProviderPatch {
                        title: Some("renamed".into()),
                        ..Default::default()
                    }
                )
                .unwrap()
                .up_next
        );
        assert!(
            !github
                .update(
                    "42",
                    now.clone(),
                    ProviderPatch {
                        up_next: Some(false),
                        ..Default::default()
                    }
                )
                .unwrap()
                .up_next
        );
        assert!(
            github
                .update(
                    "42",
                    now.clone(),
                    ProviderPatch {
                        up_next: Some(true),
                        ..Default::default()
                    }
                )
                .unwrap()
                .up_next
        );
        assert!(
            !github
                .close("42", now, CloseReason::Completed, None)
                .unwrap()
                .up_next
        );
        for index in [1, 3, 5, 7, 9] {
            let labels = label_names(&patch_body(&transport, index));
            assert_eq!(
                labels.contains(&"up-next".to_string()),
                index < 5 || index == 7
            );
            assert!(labels.contains(&"customer".to_string()));
        }
        assert!(!is_provider_label("up-next-extra"));
    }

    #[test]
    fn up_next_query_reads_only_active_labeled_issues() {
        let mut queued = issue(42, "queued", "details");
        queued["labels"] = json!([{"name":"up-next"}, {"name":"customer"}]);
        let mut closed = queued.clone();
        closed["number"] = json!(43);
        closed["state"] = json!("closed");
        closed["closed_at"] = json!("2026-08-27T00:00:00Z");
        let issues = json!([issue(41, "plain", "details"), queued, closed]);
        let transport =
            FakeTransport::with(vec![response(200, issues.clone()), response(200, issues)]);
        let github = provider(transport);
        let query = TicketQuery {
            up_next_only: true,
            ..Default::default()
        };
        let rows = github.query(&query).unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| row.native_id.as_str())
                .collect::<Vec<_>>(),
            ["42"]
        );
        assert!(rows[0].tags.contains(&"customer".to_string()));
        assert!(!rows[0].tags.contains(&"up-next".to_string()));
        let page = github.query_page(&query, None, 100).unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].native_id, "42");
    }

    /// HS2-5YNASC: a scored note is written with a `Confidence: NN%` trailer and read back
    /// into `confidence`; a human comment ending in the same words is never a score.
    #[test]
    fn note_confidence_round_trips_through_a_comment_trailer() {
        let note_id = hotsheet_model::Ulid::new();
        let written = note_trailer::compose_comment(
            "## Result\nDone.",
            Some(hotsheet_model::Confidence::new(82).unwrap()),
            note_id,
        );
        let transport = FakeTransport::with(vec![
            response(200, json!([])),
            response(201, json!({"id": 91})),
            response(200, closed_issue(42, "completed", &[])),
            response(
                200,
                json!([
                    {"id":91,"body":written,"created_at":"2026-08-26T00:02:00Z"},
                    {"id":92,"body":"Agreed.\n\nConfidence: 99%","created_at":"2026-08-26T00:03:00Z"}
                ]),
            ),
            // A scored completed issue reads its reopen history (HS2-N3RMTV): none here.
            response(
                200,
                json!([{"event":"closed","created_at":"2026-08-27T00:00:00Z"}]),
            ),
        ]);
        let github = provider(transport.clone());
        assert!(github.supports_note_confidence());
        let ticket = github
            .add_note_with_metadata(
                "42",
                MutationContext {
                    now: Timestamp::new("2026-08-26T00:02:00Z"),
                    generated_id: note_id,
                },
                NoteKind::Regular,
                NoteMetadataInput {
                    summary: None,
                    confidence: Some(hotsheet_model::Confidence::new(82).unwrap()),
                    actor: None,
                },
                "## Result\nDone.".into(),
            )
            .unwrap();
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[1].0, "POST");
        assert_eq!(
            requests[1].3.as_ref().unwrap()["body"],
            format!("## Result\nDone.\n\nConfidence: 82%\n\n<!-- hotsheet-note-id:{note_id} -->")
        );
        assert_eq!(ticket.notes[0].text, "## Result\nDone.");
        assert_eq!(ticket.notes[0].confidence, Some(82));
        assert_eq!(ticket.notes[1].text, "Agreed.\n\nConfidence: 99%");
        assert_eq!(ticket.notes[1].confidence, None);
        assert_eq!(ticket.latest_confidence, Some(82));
        assert!(
            requests[4]
                .1
                .ends_with("/issues/42/events?per_page=100&page=1")
        );
    }

    fn scored_comment(id: u64, score: u64, created_at: &str) -> Value {
        let body = note_trailer::compose_comment(
            "Done.",
            Some(hotsheet_model::Confidence::new(score).unwrap()),
            hotsheet_model::Ulid::new(),
        );
        json!({"id": id, "body": body, "created_at": created_at})
    }

    fn event(event: &str, created_at: &str) -> Value {
        json!({"event": event, "created_at": created_at})
    }

    /// HS2-N3RMTV: complete (scored) → reopen → re-complete without a new score reports no
    /// confidence; a score written after the reopen wins. The `reopened` event sits on the
    /// second events page, so the history is read to its end.
    #[test]
    fn latest_confidence_is_bounded_by_the_latest_reopen_event() {
        let full_page = (0..100)
            .map(|index| event("labeled", &format!("2026-08-26T00:00:{:02}Z", index % 60)))
            .collect::<Vec<_>>();
        let history = |tail: Vec<Value>| {
            vec![
                response(200, json!(full_page)),
                response(
                    200,
                    json!(
                        [
                            vec![
                                event("closed", "2026-08-26T01:00:00Z"),
                                event("reopened", "2026-08-26T02:00:00Z"),
                            ],
                            tail
                        ]
                        .concat()
                    ),
                ),
            ]
        };
        let old_score = scored_comment(91, 80, "2026-08-26T00:30:00Z");
        let unscored = json!({"id":92,"body":"Back on it.","created_at":"2026-08-26T02:30:00Z"});
        let mut responses = vec![
            response(200, closed_issue(42, "completed", &[])),
            response(200, json!([old_score.clone(), unscored.clone()])),
        ];
        responses.extend(history(vec![event("closed", "2026-08-26T03:00:00Z")]));
        responses.push(response(200, closed_issue(42, "completed", &[])));
        responses.push(response(
            200,
            json!([
                old_score,
                unscored,
                scored_comment(93, 95, "2026-08-26T02:45:00Z")
            ]),
        ));
        responses.extend(history(vec![event("closed", "2026-08-26T03:00:00Z")]));
        let transport = FakeTransport::with(responses);
        let github = provider(transport.clone());

        let recompleted = github.get("42").unwrap();
        assert_eq!(recompleted.notes[0].confidence, Some(80));
        assert_eq!(
            recompleted.latest_confidence, None,
            "the pre-reopen score does not survive the re-completion"
        );
        assert_eq!(github.get("42").unwrap().latest_confidence, Some(95));
        let requests = transport.requests.lock().unwrap();
        assert!(
            requests[2]
                .1
                .ends_with("/issues/42/events?per_page=100&page=1")
        );
        assert!(
            requests[3]
                .1
                .ends_with("/issues/42/events?per_page=100&page=2")
        );
        assert_eq!(requests.len(), 8);
    }

    fn plain_comments(from: u64, count: u64) -> Vec<Value> {
        (from..from + count)
            .map(|id| json!({"id": id, "body": format!("note {id}"), "created_at": "2026-08-26T00:00:00Z"}))
            .collect()
    }

    fn note_ctx(id: hotsheet_model::Ulid) -> MutationContext {
        MutationContext {
            now: Timestamp::new("2026-08-27T00:00:00Z"),
            generated_id: id,
        }
    }

    /// HS2-9GS5TS: comments past the first 100 (the newest, in ascending order) are read, so
    /// the latest scored note still sets `latest_confidence`.
    #[test]
    fn detail_read_follows_comment_pages_to_the_newest_scored_note() {
        let transport = FakeTransport::with(vec![
            response(200, closed_issue(42, "completed", &[])),
            response(200, json!(plain_comments(1, 100))),
            response(
                200,
                json!([scored_comment(101, 90, "2026-08-26T05:00:00Z")]),
            ),
            response(200, json!([])),
        ]);
        let ticket = provider(transport.clone()).get("42").unwrap();
        assert_eq!(ticket.notes.len(), 101);
        assert_eq!(ticket.notes[100].confidence, Some(90));
        assert_eq!(ticket.latest_confidence, Some(90));
        let requests = transport.requests.lock().unwrap();
        assert!(
            requests[1]
                .1
                .ends_with("/issues/42/comments?per_page=100&page=1")
        );
        assert!(
            requests[2]
                .1
                .ends_with("/issues/42/comments?per_page=100&page=2")
        );
        assert!(
            requests[3]
                .1
                .ends_with("/issues/42/events?per_page=100&page=1")
        );
        assert_eq!(requests.len(), 4);
    }

    /// Page boundaries: exactly one full page needs one empty follow-up; a short page ends it.
    #[test]
    fn comment_paging_stops_at_a_short_or_empty_page() {
        let transport = FakeTransport::with(vec![
            response(200, issue(42, "full page", "details")),
            response(200, json!(plain_comments(1, 100))),
            response(200, json!([])),
            response(200, issue(42, "short page", "details")),
            response(200, json!(plain_comments(1, 99))),
        ]);
        let github = provider(transport.clone());
        assert_eq!(github.get("42").unwrap().notes.len(), 100);
        assert_eq!(github.get("42").unwrap().notes.len(), 99);
        let requests = transport.requests.lock().unwrap();
        assert!(
            requests[2]
                .1
                .ends_with("/issues/42/comments?per_page=100&page=2")
        );
        assert_eq!(requests.len(), 5);
    }

    /// The `add_note` retry finds its idempotency marker on the second page and posts nothing.
    #[test]
    fn add_note_retry_finds_its_marker_past_the_first_comment_page() {
        let note_id = hotsheet_model::Ulid::new();
        let marker = json!({
            "id": 101,
            "body": format!("Done.\n\n{}", note_trailer::note_marker(note_id)),
            "created_at": "2026-08-26T05:00:00Z"
        });
        let transport = FakeTransport::with(vec![
            response(200, json!(plain_comments(1, 100))),
            response(200, json!([marker.clone()])),
            response(200, issue(42, "retried", "details")),
            response(200, json!(plain_comments(1, 100))),
            response(200, json!([marker])),
        ]);
        let ticket = provider(transport.clone())
            .add_note("42", note_ctx(note_id), NoteKind::Regular, "Done.".into())
            .unwrap();
        assert_eq!(ticket.notes.len(), 101);
        let requests = transport.requests.lock().unwrap();
        assert!(
            requests.iter().all(|request| request.0 == "GET"),
            "no duplicate POST"
        );
        assert_eq!(requests.len(), 5);
    }
    /// An open issue, or a closed one without a scored comment, never pays for history.
    #[test]
    fn unscored_or_open_issues_skip_the_history_request() {
        let transport = FakeTransport::with(vec![
            response(200, issue(42, "open widget", "details")),
            response(200, json!([scored_comment(91, 80, "2026-08-26T00:30:00Z")])),
            response(200, closed_issue(42, "completed", &[])),
            response(
                200,
                json!([{"id":92,"body":"plain","created_at":"2026-08-26T00:30:00Z"}]),
            ),
        ]);
        let github = provider(transport.clone());
        assert_eq!(github.get("42").unwrap().latest_confidence, None);
        assert_eq!(github.get("42").unwrap().latest_confidence, None);
        assert_eq!(transport.requests.lock().unwrap().len(), 4);
    }

    fn closed_issue(number: u64, state_reason: &str, labels: &[&str]) -> Value {
        let mut value = issue(number, "closed widget", "details");
        value["state"] = json!("closed");
        value["state_reason"] = json!(state_reason);
        value["closed_at"] = json!("2026-08-27T00:00:00Z");
        value["labels"] = json!(
            labels
                .iter()
                .map(|name| json!({"name": name}))
                .collect::<Vec<_>>()
        );
        value
    }

    fn patch_body(transport: &FakeTransport, index: usize) -> Value {
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[index].0, "PATCH", "request {index} is a PATCH");
        requests[index].3.clone().expect("PATCH body")
    }

    fn label_names(body: &Value) -> Vec<String> {
        body["labels"]
            .as_array()
            .expect("labels array")
            .iter()
            .map(|label| label.as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn closed_issues_read_status_and_exact_close_reason_from_provider_labels() {
        // HS2-K8R3T8: GitHub only has open/closed plus completed/not_planned, so Verified,
        // Archived, Duplicate, Obsolete, and Works as designed ride on provider-owned labels.
        type Case = (
            &'static str,
            &'static [&'static str],
            Status,
            CloseReason,
            Option<&'static str>,
        );
        let cases: [Case; 6] = [
            (
                "completed",
                &["category:bug"],
                Status::Completed,
                CloseReason::Completed,
                None,
            ),
            (
                "not_planned",
                &["category:bug"],
                Status::Completed,
                CloseReason::NotPlanned,
                None,
            ),
            (
                "not_planned",
                &[
                    "category:bug",
                    "status:verified",
                    "closed:works-as-designed",
                    "customer",
                ],
                Status::Verified,
                CloseReason::WorksAsDesigned,
                None,
            ),
            (
                "completed",
                &["category:bug", "status:archived"],
                Status::Archive,
                CloseReason::Completed,
                None,
            ),
            (
                "not_planned",
                &[
                    "category:bug",
                    "closed:duplicate",
                    "duplicate-of:acme/widgets#7",
                ],
                Status::Completed,
                CloseReason::Duplicate,
                Some("acme/widgets#7"),
            ),
            (
                "not_planned",
                &["category:bug", "closed:obsolete", "status:verified"],
                Status::Verified,
                CloseReason::Obsolete,
                None,
            ),
        ];
        for (state_reason, labels, status, reason, duplicate_of) in cases {
            let transport = FakeTransport::with(vec![
                response(200, closed_issue(9, state_reason, labels)),
                response(200, json!([])),
            ]);
            let ticket = provider(transport).get("9").unwrap();
            assert_eq!(ticket.status, status, "{labels:?}");
            assert_eq!(ticket.close_reason, Some(reason), "{labels:?}");
            assert_eq!(ticket.duplicate_of.as_deref(), duplicate_of, "{labels:?}");
            assert_eq!(
                ticket.verified_at.as_deref(),
                (status == Status::Verified).then_some("2026-08-27T00:00:00Z"),
                "{labels:?}"
            );
            assert!(
                ticket.tags.iter().all(|tag| !tag.contains(':')),
                "provider labels never surface as tags: {:?}",
                ticket.tags
            );
        }
        // An open issue ignores stale closed-only labels and reads its open-only label.
        let mut open = issue(10, "open widget", "details");
        open["labels"] = json!([{"name":"status:verified"},{"name":"status:started"},{"name":"closed:obsolete"}]);
        let transport = FakeTransport::with(vec![response(200, open), response(200, json!([]))]);
        let ticket = provider(transport).get("10").unwrap();
        assert_eq!(ticket.status, Status::Started);
        assert_eq!(ticket.close_reason, None);
        assert_eq!(ticket.duplicate_of, None);
    }

    #[test]
    fn verifying_a_closed_issue_keeps_its_close_reason_and_labels_it_verified() {
        // The reported bug: moving a Works-as-designed ticket to Verified used to PATCH
        // without `state_reason` (GitHub then reset it to completed) and read back as
        // Completed because Verified had no representation.
        let before = closed_issue(
            42,
            "not_planned",
            &[
                "category:bug",
                "priority:high",
                "customer",
                "closed:works-as-designed",
            ],
        );
        let mut after = before.clone();
        after["labels"] = json!([
            {"name":"category:bug"},{"name":"priority:high"},{"name":"customer"},
            {"name":"closed:works-as-designed"},{"name":"status:verified"}
        ]);
        let transport = FakeTransport::with(vec![response(200, before), response(200, after)]);
        let ticket = provider(transport.clone())
            .update(
                "42",
                Timestamp::new("2026-08-28T00:00:00Z"),
                ProviderPatch {
                    status: Some(Status::Verified),
                    ..Default::default()
                },
            )
            .unwrap();
        let body = patch_body(&transport, 1);
        assert_eq!(body["state"], "closed");
        assert_eq!(body["state_reason"], "not_planned");
        assert_eq!(
            label_names(&body),
            [
                "category:bug",
                "closed:works-as-designed",
                "customer",
                "priority:high",
                "status:verified"
            ]
        );
        assert_eq!(ticket.status, Status::Verified);
        assert_eq!(ticket.close_reason, Some(CloseReason::WorksAsDesigned));
    }

    #[test]
    fn close_records_the_exact_reason_and_duplicate_target_while_keeping_tags() {
        let open = issue(42, "dup widget", "details");
        let mut closed = closed_issue(
            42,
            "not_planned",
            &[
                "category:bug",
                "priority:high",
                "customer",
                "closed:duplicate",
                "duplicate-of:@main/github-main:7",
            ],
        );
        closed["title"] = json!("dup widget");
        let transport = FakeTransport::with(vec![response(200, open), response(200, closed)]);
        let ticket = provider(transport.clone())
            .close(
                "42",
                Timestamp::new("2026-08-28T00:00:00Z"),
                CloseReason::Duplicate,
                Some("@main/github-main:7".into()),
            )
            .unwrap();
        let body = patch_body(&transport, 1);
        assert_eq!(body["state"], "closed");
        assert_eq!(body["state_reason"], "not_planned");
        assert_eq!(
            label_names(&body),
            [
                "category:bug",
                "closed:duplicate",
                "customer",
                "duplicate-of:@main/github-main:7",
                "priority:high"
            ]
        );
        assert_eq!(ticket.status, Status::Completed);
        assert_eq!(ticket.close_reason, Some(CloseReason::Duplicate));
        assert_eq!(ticket.duplicate_of.as_deref(), Some("@main/github-main:7"));
        assert_eq!(ticket.tags, ["customer"]);
    }

    #[test]
    fn close_verify_reopen_and_reclose_walk_the_status_transitions_without_leaking_labels() {
        // Transition matrix across the closed/open boundary: each PATCH must carry exactly
        // the labels and reason for its target state, and reopening must drop closed-only
        // labels so the next close starts clean.
        let base = ["category:bug", "priority:high", "customer"];
        let step = |state: &str, reason: &str, extra: &[&str]| {
            let labels = base.iter().chain(extra).copied().collect::<Vec<_>>();
            if state == "open" {
                let mut value = issue(42, "widget", "details");
                value["labels"] = json!(
                    labels
                        .iter()
                        .map(|name| json!({"name": name}))
                        .collect::<Vec<_>>()
                );
                value
            } else {
                closed_issue(42, reason, &labels)
            }
        };
        let transport = FakeTransport::with(vec![
            // 1. close as works-as-designed: GET current, PATCH
            response(200, step("open", "", &[])),
            response(
                200,
                step("closed", "not_planned", &["closed:works-as-designed"]),
            ),
            // 2. verify: GET, PATCH
            response(
                200,
                step("closed", "not_planned", &["closed:works-as-designed"]),
            ),
            response(
                200,
                step(
                    "closed",
                    "not_planned",
                    &["closed:works-as-designed", "status:verified"],
                ),
            ),
            // 3. reopen as started: GET, PATCH
            response(
                200,
                step(
                    "closed",
                    "not_planned",
                    &["closed:works-as-designed", "status:verified"],
                ),
            ),
            response(200, step("open", "", &["status:started"])),
            // 4. mark completed through the status patch: GET, PATCH
            response(200, step("open", "", &["status:started"])),
            response(200, step("closed", "completed", &[])),
            // 5. archive: GET, PATCH
            response(200, step("closed", "completed", &[])),
            response(200, step("closed", "completed", &["status:archived"])),
        ]);
        let provider = provider(transport.clone());
        let now = Timestamp::new("2026-08-28T00:00:00Z");
        let status_patch = |status: Status| ProviderPatch {
            status: Some(status),
            ..Default::default()
        };

        let closed = provider
            .close("42", now.clone(), CloseReason::WorksAsDesigned, None)
            .unwrap();
        assert_eq!(closed.close_reason, Some(CloseReason::WorksAsDesigned));
        let body = patch_body(&transport, 1);
        assert_eq!(body["state_reason"], "not_planned");
        assert!(label_names(&body).contains(&"closed:works-as-designed".to_string()));

        let verified = provider
            .update("42", now.clone(), status_patch(Status::Verified))
            .unwrap();
        assert_eq!(verified.status, Status::Verified);
        assert_eq!(verified.close_reason, Some(CloseReason::WorksAsDesigned));
        let body = patch_body(&transport, 3);
        assert_eq!(body["state"], "closed");
        assert_eq!(body["state_reason"], "not_planned");
        assert_eq!(
            label_names(&body),
            [
                "category:bug",
                "closed:works-as-designed",
                "customer",
                "priority:high",
                "status:verified"
            ]
        );

        let reopened = provider
            .update("42", now.clone(), status_patch(Status::Started))
            .unwrap();
        assert_eq!(reopened.status, Status::Started);
        assert_eq!(reopened.close_reason, None);
        let body = patch_body(&transport, 5);
        assert_eq!(body["state"], "open");
        assert!(
            body.get("state_reason").is_none(),
            "reopen sends no state_reason"
        );
        assert_eq!(
            label_names(&body),
            [
                "category:bug",
                "customer",
                "priority:high",
                "status:started"
            ]
        );

        let completed = provider
            .update("42", now.clone(), status_patch(Status::Completed))
            .unwrap();
        assert_eq!(completed.status, Status::Completed);
        assert_eq!(completed.close_reason, Some(CloseReason::Completed));
        let body = patch_body(&transport, 7);
        assert_eq!(body["state"], "closed");
        assert_eq!(
            body["state_reason"], "completed",
            "a status close starts clean"
        );
        assert_eq!(
            label_names(&body),
            ["category:bug", "customer", "priority:high"]
        );

        let archived = provider
            .update("42", now, status_patch(Status::Archive))
            .unwrap();
        assert_eq!(archived.status, Status::Archive);
        let body = patch_body(&transport, 9);
        assert_eq!(body["state_reason"], "completed");
        assert_eq!(
            label_names(&body),
            [
                "category:bug",
                "customer",
                "priority:high",
                "status:archived"
            ]
        );
        assert_eq!(transport.requests.lock().unwrap().len(), 10);
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
            "<https://api.test/repos/acme/widgets/issues?page2>; rel=\"next\"".into(),
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
    fn live_read_state_coalesces_issue_walks_and_honors_rate_limit_cooldown() {
        let mut limited = response(403, json!({"message":"secondary rate limit exceeded"}));
        limited.headers.insert("retry-after".into(), "60".into());
        let transport = FakeTransport::with(vec![
            response(200, json!([issue(1, "one", "")])),
            response(201, issue(2, "two", "")),
            response(200, json!([issue(2, "two", "")])),
            limited,
            response(200, json!([issue(3, "three", "")])),
        ]);
        let mut provider = provider(transport.clone());
        provider.issues_cache = Some(Arc::new(Mutex::new(None)));
        provider.rate_limited_until = Some(Arc::new(Mutex::new(None)));

        assert_eq!(provider.query(&TicketQuery::default()).unwrap().len(), 1);
        assert_eq!(provider.query(&TicketQuery::default()).unwrap().len(), 1);
        assert_eq!(transport.requests.lock().unwrap().len(), 1);
        provider
            .request(
                "POST",
                &provider.endpoint("issues"),
                Some(&json!({"title":"two"})),
            )
            .unwrap();
        assert_eq!(
            provider.query(&TicketQuery::default()).unwrap()[0].title,
            "two"
        );
        assert_eq!(transport.requests.lock().unwrap().len(), 3);
        *provider.issues_cache.as_ref().unwrap().lock().unwrap() = None;
        assert!(matches!(
            provider.query(&TicketQuery::default()),
            Err(ProviderError::RateLimited { .. })
        ));
        assert!(matches!(
            provider.query(&TicketQuery::default()),
            Err(ProviderError::RateLimited { .. })
        ));
        assert_eq!(transport.requests.lock().unwrap().len(), 4);
        *provider
            .rate_limited_until
            .as_ref()
            .unwrap()
            .lock()
            .unwrap() = None;
        assert_eq!(
            provider.query(&TicketQuery::default()).unwrap()[0].title,
            "three"
        );
        assert_eq!(transport.requests.lock().unwrap().len(), 5);
    }

    #[test]
    fn native_page_returns_the_remote_cursor_without_materializing_following_pages() {
        let mut first = response(200, json!([issue(1, "one", "")]));
        first.headers.insert(
            "link".into(),
            "<https://api.test/repos/acme/widgets/issues?page2>; rel=\"next\"".into(),
        );
        let transport = FakeTransport::with(vec![first]);
        let provider = provider(transport.clone());
        assert!(
            provider
                .query_page(
                    &TicketQuery::default(),
                    Some("https://evil.test/issues?page=2"),
                    100
                )
                .is_err()
        );
        let page = provider
            .query_page(&TicketQuery::default(), None, 100)
            .unwrap();
        assert_eq!(
            page.items
                .iter()
                .map(|ticket| ticket.native_id.as_str())
                .collect::<Vec<_>>(),
            ["1"]
        );
        assert_eq!(
            page.next_cursor.as_deref(),
            Some("https://api.test/repos/acme/widgets/issues?page2")
        );
        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn text_search_matches_list_fields_with_all_terms_and_word_prefixes() {
        let transport = FakeTransport::with(vec![
            response(
                200,
                json!([
                    issue(1, "Broken parser", "fails on import"),
                    issue(2, "Healthy parser", "handles imports")
                ]),
            ),
            response(
                200,
                json!([
                    issue(1, "Broken parser", "fails on import"),
                    issue(2, "Healthy parser", "handles imports")
                ]),
            ),
        ]);
        let provider = provider(transport);
        let query = TicketQuery {
            text: Some("bro par".into()),
            ..TicketQuery::default()
        };
        let matches = provider.query(&query).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].native_id, "1");
        for text in ["imp", "cus", "widgets #1", "  "] {
            assert!(provider_text_matches(&matches[0], Some(text)), "{text}");
        }
        assert!(!provider_text_matches(&matches[0], Some("missing")));
        let page = provider.query_page(&query, None, 100).unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].native_id, "1");
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
        // HS2-5YNASC: the confidence trailer round-trips through real GitHub comments.
        let commented = provider
            .add_note_with_metadata(
                &created.native_id,
                MutationContext {
                    now: Timestamp::new("2026-08-26T00:01:00Z"),
                    generated_id: hotsheet_model::Ulid::new(),
                },
                NoteKind::Regular,
                NoteMetadataInput {
                    summary: None,
                    confidence: Some(hotsheet_model::Confidence::new(77).unwrap()),
                    actor: None,
                },
                "Hot Sheet live comment validation".into(),
            )
            .unwrap();
        let note = commented.notes.last().unwrap();
        assert_eq!(note.text, "Hot Sheet live comment validation");
        assert_eq!(note.confidence, Some(77));
        let closed = provider
            .close(
                &created.native_id,
                Timestamp::new("2026-08-26T00:02:00Z"),
                CloseReason::Completed,
                None,
            )
            .unwrap();
        assert_eq!(closed.close_reason, Some(CloseReason::Completed));
        // HS2-7D9BPK: the reopen-history bound against real issue events. Closed with the
        // scored comment above, the score is current; reopen and re-close with no new score
        // and it is gone; a score written after the reopen is reported.
        assert_eq!(
            provider.get(&created.native_id).unwrap().latest_confidence,
            Some(77)
        );
        let second = std::time::Duration::from_millis(1100); // GitHub timestamps are whole seconds
        let status = |status| ProviderPatch {
            status: Some(status),
            ..Default::default()
        };
        std::thread::sleep(second);
        provider
            .update(
                &created.native_id,
                Timestamp::new("2026-08-26T00:03:00Z"),
                status(hotsheet_model::Status::NotStarted),
            )
            .unwrap();
        std::thread::sleep(second);
        provider
            .close(
                &created.native_id,
                Timestamp::new("2026-08-26T00:04:00Z"),
                CloseReason::Completed,
                None,
            )
            .unwrap();
        assert_eq!(
            provider.get(&created.native_id).unwrap().latest_confidence,
            None,
            "a reopen discards the earlier cycle's score"
        );
        let rescored = provider
            .add_note_with_metadata(
                &created.native_id,
                MutationContext {
                    now: Timestamp::new("2026-08-26T00:05:00Z"),
                    generated_id: hotsheet_model::Ulid::new(),
                },
                NoteKind::Regular,
                NoteMetadataInput {
                    summary: None,
                    confidence: Some(hotsheet_model::Confidence::new(88).unwrap()),
                    actor: None,
                },
                "Hot Sheet live re-completion validation".into(),
            )
            .unwrap();
        assert_eq!(rescored.latest_confidence, Some(88));
    }

    // ---- Attachments through an assets repository (HS2-HSA64D) ----

    fn assets_provider(transport: Arc<dyn GitHubTransport>) -> GitHubProvider {
        let mut config = GitHubConfig::new("github-main", "acme/widgets", "test-token")
            .with_attachments(GitHubAttachmentRepository::new("acme/assets", None, None).unwrap());
        config.api_base = "https://api.test".into();
        GitHubProvider::new(config, transport)
    }

    fn evidence(id: &str, filename: &str) -> ApiAttachment {
        ApiAttachment {
            id: id.into(),
            filename: filename.into(),
            created_at: "2026-10-01T00:00:00Z".into(),
            batch_id: Some("batch-1".into()),
            batch_label: Some("Repro".into()),
            actor: Some(hotsheet_model::AttachmentActor {
                identity: None,
                display_name: None,
                role: hotsheet_model::AttachmentActorRole::Human,
            }),
            purpose: Some(hotsheet_model::AttachmentPurpose::ProblemEvidence),
            annotations: vec![],
        }
    }

    const ATTACHMENT_ID: &str = "01K6ATTACHMENT0000000000AB";

    fn content_file(path: &str) -> Value {
        json!({
            "path": path,
            "sha": "blobsha1",
            "html_url": format!("https://ghe.test/acme/assets/blob/main/{path}"),
        })
    }

    fn uploaded_comment(id: u64, attachment_id: &str, filename: &str) -> Value {
        let marker = AttachmentMarker {
            filename: filename.into(),
            path: format!("hotsheet-attachments/{attachment_id}-{filename}"),
            repository: "acme/assets".into(),
            branch: "main".into(),
            sha: Some("blobsha1".into()),
            batch_id: Some("batch-1".into()),
            batch_label: Some("Repro".into()),
            actor: None,
            purpose: Some(hotsheet_model::AttachmentPurpose::ProblemEvidence),
        };
        json!({
            "id": id,
            "body": github_attachments::compose_comment(attachment_id, "https://x/y", &marker),
            "created_at": "2026-10-01T00:00:05Z",
        })
    }

    #[test]
    fn attachments_are_unsupported_without_an_assets_repository() {
        let transport = FakeTransport::with(vec![]);
        let github = provider(transport.clone());
        let capabilities = github.descriptor().capabilities;
        assert!(!capabilities.attachments);
        assert!(!capabilities.attachment_edit);
        let error = github
            .add_attachment("42", evidence(ATTACHMENT_ID, "a.txt"), b"x".to_vec())
            .unwrap_err();
        assert!(matches!(
            error,
            ProviderError::Unsupported {
                capability: "attachments",
                ..
            }
        ));
        assert!(transport.requests.lock().unwrap().is_empty());
        let assets = assets_provider(FakeTransport::with(vec![]));
        assert!(assets.descriptor().capabilities.attachments);
        assert!(!assets.descriptor().capabilities.attachment_edit);
    }

    #[test]
    fn add_attachment_commits_to_the_assets_repository_and_links_it_from_a_comment() {
        let path = format!("hotsheet-attachments/{ATTACHMENT_ID}-shot_1.png");
        let transport = FakeTransport::with(vec![
            response(200, json!([])),
            response(201, json!({ "content": content_file(&path) })),
            response(201, json!({"id": 7})),
            response(200, issue(42, "broken widget", "details")),
            response(
                200,
                json!([
                    {"id": 5, "body": "a human note", "created_at": "2026-10-01T00:00:01Z"},
                    uploaded_comment(6, ATTACHMENT_ID, "shot_1.png"),
                ]),
            ),
        ]);
        let ticket = assets_provider(transport.clone())
            .add_attachment(
                "42",
                evidence(ATTACHMENT_ID, "shot 1.png"),
                vec![0, 159, 255],
            )
            .unwrap();
        let requests = transport.requests.lock().unwrap();
        let (method, url, _, body) = &requests[1];
        assert_eq!(method, "PUT");
        assert_eq!(
            url,
            &format!("https://api.test/repos/acme/assets/contents/{path}")
        );
        let body = body.as_ref().unwrap();
        assert_eq!(body["branch"], "main");
        assert_eq!(body["content"], "AJ//");
        assert_eq!(
            body["message"],
            "Upload attachment: shot 1.png (acme/widgets#42)"
        );
        let (method, url, _, comment) = &requests[2];
        assert_eq!(method, "POST");
        assert_eq!(
            url,
            "https://api.test/repos/acme/widgets/issues/42/comments"
        );
        let comment = comment.as_ref().unwrap()["body"]
            .as_str()
            .unwrap()
            .to_owned();
        // A non-github.com API base links the Enterprise file page's raw form.
        assert!(
            comment.starts_with(&format!(
                "![shot 1.png](https://ghe.test/acme/assets/raw/main/{path})"
            )),
            "{comment}"
        );
        let (id, marker) = github_attachments::parse_comment(&comment).unwrap();
        assert_eq!(id, ATTACHMENT_ID);
        assert_eq!(marker.sha.as_deref(), Some("blobsha1"));
        assert_eq!(marker.batch_label.as_deref(), Some("Repro"));
        // The link comment projects as an attachment and is not repeated as a note.
        assert_eq!(ticket.notes.len(), 1);
        assert_eq!(ticket.notes[0].text, "a human note");
        assert_eq!(ticket.attachments.len(), 1);
        let attachment = &ticket.attachments[0];
        assert_eq!(attachment.id, ATTACHMENT_ID);
        assert_eq!(attachment.filename, "shot_1.png");
        assert_eq!(attachment.created_at, "2026-10-01T00:00:05Z");
        assert_eq!(attachment.batch_id.as_deref(), Some("batch-1"));
        assert_eq!(
            attachment.purpose,
            Some(hotsheet_model::AttachmentPurpose::ProblemEvidence)
        );
    }

    #[test]
    fn github_com_links_use_the_permanent_raw_url() {
        let path = format!("hotsheet-attachments/{ATTACHMENT_ID}-notes.txt");
        let transport = FakeTransport::with(vec![
            response(200, json!([])),
            response(201, json!({ "content": content_file(&path) })),
            response(201, json!({"id": 7})),
            response(200, issue(42, "broken widget", "details")),
            response(200, json!([])),
        ]);
        let config = GitHubConfig::new("github-main", "acme/widgets", "t")
            .with_attachments(GitHubAttachmentRepository::new("acme/assets", None, None).unwrap());
        GitHubProvider::new(config, transport.clone())
            .add_attachment("42", evidence(ATTACHMENT_ID, "notes.txt"), b"hi".to_vec())
            .unwrap();
        let requests = transport.requests.lock().unwrap();
        assert!(
            requests[1]
                .1
                .starts_with("https://api.github.com/repos/acme/assets/contents/")
        );
        let comment = requests[2].3.as_ref().unwrap()["body"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(comment.starts_with(&format!(
            "[notes.txt](https://raw.githubusercontent.com/acme/assets/main/{path})"
        )));
    }

    #[test]
    fn a_retried_attachment_is_not_uploaded_or_linked_twice() {
        // Already linked: only the read happens.
        let linked = FakeTransport::with(vec![
            response(200, json!([uploaded_comment(6, ATTACHMENT_ID, "a.txt")])),
            response(200, issue(42, "broken widget", "details")),
            response(200, json!([uploaded_comment(6, ATTACHMENT_ID, "a.txt")])),
        ]);
        let ticket = assets_provider(linked.clone())
            .add_attachment("42", evidence(ATTACHMENT_ID, "a.txt"), b"x".to_vec())
            .unwrap();
        assert_eq!(ticket.attachments.len(), 1);
        assert!(
            linked
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|(method, ..)| method == "GET")
        );
        // Uploaded but never linked (the comment failed): the existing file is reused.
        let path = format!("hotsheet-attachments/{ATTACHMENT_ID}-a.txt");
        let half = FakeTransport::with(vec![
            response(200, json!([])),
            response(422, json!({"message": "\"sha\" wasn't supplied."})),
            response(200, content_file(&path)),
            response(201, json!({"id": 8})),
            response(200, issue(42, "broken widget", "details")),
            response(200, json!([uploaded_comment(8, ATTACHMENT_ID, "a.txt")])),
        ]);
        let ticket = assets_provider(half.clone())
            .add_attachment("42", evidence(ATTACHMENT_ID, "a.txt"), b"x".to_vec())
            .unwrap();
        assert_eq!(ticket.attachments.len(), 1);
        let requests = half.requests.lock().unwrap();
        assert_eq!(
            requests[2].1,
            format!("https://api.test/repos/acme/assets/contents/{path}?ref=main")
        );
        assert_eq!(requests[3].0, "POST");
    }

    #[test]
    fn a_failed_upload_surfaces_the_github_error_and_posts_no_comment() {
        let transport = FakeTransport::with(vec![
            response(200, json!([])),
            response(422, json!({"message": "Branch media not found"})),
            response(404, json!({"message": "Not Found"})),
        ]);
        let error = assets_provider(transport.clone())
            .add_attachment("42", evidence(ATTACHMENT_ID, "a.txt"), b"x".to_vec())
            .unwrap_err();
        assert!(
            error.to_string().contains("Branch media not found"),
            "{error}"
        );
        assert_eq!(transport.requests.lock().unwrap().len(), 3);
        let invalid = assets_provider(FakeTransport::with(vec![]))
            .add_attachment("42", evidence("../x", "a.txt"), b"x".to_vec())
            .unwrap_err();
        assert!(matches!(invalid, ProviderError::InvalidNativeId { .. }));
    }

    #[test]
    fn attachment_bytes_read_the_blob_through_the_authenticated_api() {
        let transport = FakeTransport::with(vec![
            response(200, json!([uploaded_comment(6, ATTACHMENT_ID, "a.bin")])),
            response(
                200,
                json!({"content": "AJ//\nAA==\n", "encoding": "base64"}),
            ),
            response(200, json!([uploaded_comment(6, ATTACHMENT_ID, "a.bin")])),
        ]);
        let github = assets_provider(transport.clone());
        assert_eq!(
            github.attachment_bytes("42", ATTACHMENT_ID).unwrap(),
            vec![0, 159, 255, 0]
        );
        assert_eq!(
            transport.requests.lock().unwrap()[1].1,
            "https://api.test/repos/acme/assets/git/blobs/blobsha1"
        );
        assert!(matches!(
            github.attachment_bytes("42", "01K6OTHER").unwrap_err(),
            ProviderError::NotFound { .. }
        ));
    }

    #[test]
    fn from_connection_reads_and_validates_the_assets_repository_settings() {
        let mut connection = ProviderConnection {
            id: "gh".into(),
            provider: "github".into(),
            locator: "acme/widgets".into(),
            name: None,
            default: false,
            settings: json!({"attachment_repo": "acme/assets", "attachment_branch": "media"}),
            disabled: false,
        };
        let config = GitHubConfig::from_connection(&connection, "t").unwrap();
        let assets = config.attachments.unwrap();
        assert_eq!(assets.folder, "hotsheet-attachments");
        assert_eq!(assets.branch, "media");
        connection.settings = json!({"attachment_repo": "not-a-repo"});
        assert!(GitHubConfig::from_connection(&connection, "t").is_err());
        connection.settings = json!({});
        assert!(
            GitHubConfig::from_connection(&connection, "t")
                .unwrap()
                .attachments
                .is_none()
        );
    }
}
