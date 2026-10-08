use std::sync::Arc;
use std::time::Instant;

use hotsheet_model::{CloseReason, NoteKind, Priority, ReviewRequest, Status, Timestamp};
use hotsheet_ticketing::ops::NoteMetadataInput;
use hotsheet_ticketing::{
    ApiNote, ApiTicket, MutationContext, ProviderCapabilities, ProviderConnection,
    ProviderDescriptor, ProviderDraft, ProviderError, ProviderKeysetPage, ProviderMutationTiming,
    ProviderPatch, ProviderTicketPage, ProviderTicketSummary, SortKey, TicketProvider, TicketQuery,
    checkout_order::MergeKey, compare_provider_tickets, filter_provider_ticket_page,
    keyset_page_from_native_pages, keyset_page_from_rows, provider_text_matches, unbounded_query,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::note_trailer;

/// Native page size for value-keyset walks; fixed so resume hints stay positionally valid.
const NATIVE_KEYSET_PAGE: usize = 100;

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

    /// When the issue left the `done` status category, from its changelog (HS2-N3RMTV).
    /// Changelog items carry status ids but not categories, so the status catalogue is read
    /// only when the changelog has a status transition at all.
    fn reopen_times(&self, key: &str) -> Result<Vec<String>, ProviderError> {
        let mut transitions = Vec::new();
        let mut start_at = 0usize;
        for _ in 0..MAX_HISTORY_PAGES {
            let response = self.request(
                "GET",
                &self.endpoint(&format!(
                    "issue/{key}/changelog?startAt={start_at}&maxResults=100"
                )),
                None,
            )?;
            let page: JiraChangelog = self.json(response)?;
            let len = page.values.len();
            for history in page.values {
                for item in history.items {
                    if item.field == "status"
                        && let (Some(from), Some(to)) = (item.from, item.to)
                    {
                        transitions.push((history.created.clone(), from, to));
                    }
                }
            }
            start_at += len;
            if page.is_last || len == 0 {
                break;
            }
        }
        if transitions.is_empty() {
            return Ok(Vec::new());
        }
        let response = self.request("GET", &self.endpoint("status"), None)?;
        let done = self
            .json::<Vec<JiraStatusEntry>>(response)?
            .into_iter()
            .filter(|status| status.status_category.key == "done")
            .map(|status| status.id)
            .collect::<std::collections::HashSet<_>>();
        Ok(transitions
            .into_iter()
            .filter(|(_, from, to)| done.contains(from) && !done.contains(to))
            .map(|(created, _, _)| created)
            .collect())
    }

    /// Every issue comment (HS2-9GS5TS), paging by `startAt` until `total` is reached, so
    /// comments past the first 100 — the newest scored note or an `add_note` idempotency
    /// marker — are never dropped. Without `total`, a short page ends the list.
    fn comments(&self, key: &str) -> Result<Vec<JiraComment>, ProviderError> {
        const PAGE: usize = 100;
        let mut comments = Vec::new();
        for _ in 0..MAX_HISTORY_PAGES {
            let start_at = comments.len();
            let response = self.request(
                "GET",
                &self.endpoint(&format!(
                    "issue/{key}/comment?startAt={start_at}&maxResults={PAGE}"
                )),
                None,
            )?;
            let page: JiraComments = self.json(response)?;
            let len = page.comments.len();
            comments.extend(page.comments);
            let done = match page.total {
                Some(total) => comments.len() >= total,
                None => len < PAGE,
            };
            if done || len == 0 {
                break;
            }
        }
        Ok(comments)
    }

    /// Reject filters the native page API cannot evaluate (shared by paged reads).
    fn check_query_filters(&self, query: &TicketQuery) -> Result<(), ProviderError> {
        if query.review_requested.is_some()
            || query.review_by.is_some()
            || query.claimed.is_some()
            || query.blocked.is_some()
            || query.page_after.is_some()
            || query.up_next_only
            || query.close_reason.is_some()
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
            return self.unsupported("requested query filter");
        }
        Ok(())
    }

    fn issue_page(
        &self,
        cursor: Option<&str>,
        updated_after: Option<&str>,
        limit: usize,
        sort: SortKey,
    ) -> Result<(Vec<JiraIssue>, Option<String>), ProviderError> {
        let order = match sort {
            SortKey::Id => "key",
            SortKey::Created => "created",
            _ => "updated",
        };
        let mut jql = format!("project = {} ORDER BY {order} ASC", self.config.project_key);
        if let Some(after) = updated_after {
            jql = format!(
                "project = {} AND updated >= \"{}\" ORDER BY {order} ASC",
                self.config.project_key, after
            )
        }
        let response=self.request("POST",&self.endpoint("search/jql"),Some(&json!({"jql":jql,"nextPageToken":cursor,"maxResults":limit.min(100),"fields":["summary","description","status","priority","issuetype","labels","assignee","created","updated","resolutiondate"]})))?;
        let page: JiraSearch = self.json(response)?;
        let next = if page.is_last {
            None
        } else {
            page.next_page_token
        };
        Ok((page.issues, next))
    }

    fn list_issues(&self, updated_after: Option<&str>) -> Result<Vec<JiraIssue>, ProviderError> {
        let mut next_page_token: Option<String> = None;
        let mut issues = Vec::new();
        loop {
            let (page, next) = self.issue_page(
                next_page_token.as_deref(),
                updated_after,
                100,
                SortKey::Updated,
            )?;
            issues.extend(page);
            let Some(next) = next else { break };
            next_page_token = Some(next);
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
        let notes = comments
            .into_iter()
            .map(|comment| {
                let (text, confidence) =
                    note_trailer::parse_comment(&adf_to_text(Some(&comment.body)));
                ApiNote {
                    id: comment.id,
                    kind: NoteKind::Regular,
                    created_at: comment.created.clone(),
                    edited_at: comment.updated.unwrap_or(comment.created),
                    summary: None,
                    confidence,
                    feedback_for: None,
                    ai_feedback: hotsheet_model::AiFeedback::from_legacy_text(&text),
                    human_edited: false,
                    actor: None,
                    text,
                }
            })
            .collect::<Vec<_>>();
        let latest_confidence = note_trailer::latest_confidence(status, &notes, None);
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
            started_phase: None,
            up_next: false,
            feedback_needed: false,
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
            claim_eta_at: None,
            claim_started_at: None,
            worker_label: None,
            legacy_number: None,
            claim_count: 0,
            claim_history: vec![],
            assignees: issue
                .fields
                .assignee
                .into_iter()
                .map(|user| user.account_id)
                .collect(),
            review_requests: vec![],
            schema: 1,
            notes,
            latest_confidence,
            attachments: vec![],
            warnings: vec![],
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
            color: None,
            connection_id: self.config.connection_id.clone(),
            provider: "jira".into(),
            display_name: format!("Jira {}", self.config.project_key),
            locator: self.config.project_key.clone(),
            default: self.config.default,
            capabilities: capabilities(),
        }
    }

    fn query(&self, query: &TicketQuery) -> Result<Vec<ApiTicket>, ProviderError> {
        if query.review_requested.is_some()
            || query.review_by.is_some()
            || query.claimed.is_some()
            || query.blocked.is_some()
            || query.page_after.is_some()
            || query.up_next_only
            || query.close_reason.is_some()
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
            return self.unsupported("requested query filter");
        }
        let mut tickets = self
            .list_issues(query.updated_after.as_deref())?
            .into_iter()
            .map(|issue| self.ticket(issue, vec![]))
            .filter(|ticket| provider_text_matches(ticket, query.text.as_deref()))
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
        if query.descending
            || !matches!(
                query.sort,
                SortKey::Id | SortKey::Created | SortKey::Updated
            )
        {
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
            self.issue_page(cursor, query.updated_after.as_deref(), limit, query.sort)?;
        Ok(ProviderTicketPage {
            items: filter_provider_ticket_page(
                issues
                    .into_iter()
                    .map(|issue| self.ticket(issue, vec![]))
                    .collect(),
                query,
            ),
            next_cursor,
        })
    }

    /// Value keyset (HS2-74H84S). Ascending creation/update order is natively keyset-safe;
    /// Jira orders issue keys numerically while the checkout `id` order compares native ids
    /// as strings, so every other sort resumes over the full filtered result.
    fn query_after(
        &self,
        query: &TicketQuery,
        after: Option<&MergeKey>,
        resume: Option<&str>,
        limit: usize,
    ) -> Result<ProviderKeysetPage, ProviderError> {
        if query.descending || !matches!(query.sort, SortKey::Created | SortKey::Updated) {
            return Ok(keyset_page_from_rows(
                self.query(&unbounded_query(query))?,
                query,
                after,
                limit,
            ));
        }
        self.check_query_filters(query)?;
        keyset_page_from_native_pages(query, after, resume, limit, |cursor| {
            let (issues, next) = self.issue_page(
                cursor,
                query.updated_after.as_deref(),
                NATIVE_KEYSET_PAGE,
                query.sort,
            )?;
            Ok((
                issues
                    .into_iter()
                    .map(|issue| self.ticket(issue, vec![]))
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
        let mut cursor = None;
        loop {
            let (issues, next) = self.issue_page(cursor.as_deref(), None, 100, SortKey::Updated)?;
            for issue in issues {
                summary.add_ticket(&self.ticket(issue, vec![]), now, day_starts)
            }
            let Some(next) = next else { break };
            cursor = Some(next)
        }
        Ok(summary)
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
        let mut ticket = self.ticket(self.issue(native_id)?, self.comments(native_id)?);
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
        now: Timestamp,
        patch: ProviderPatch,
    ) -> Result<ApiTicket, ProviderError> {
        self.update_timed(native_id, now, patch)
            .map(|(ticket, _)| ticket)
    }

    fn update_timed(
        &self,
        native_id: &str,
        _: Timestamp,
        patch: ProviderPatch,
    ) -> Result<(ApiTicket, ProviderMutationTiming), ProviderError> {
        let mut timing = ProviderMutationTiming::default();
        if patch.blocked_reason.is_some() {
            return self.unsupported("blocked_reason");
        }
        if patch.started_phase.is_some() {
            return self.unsupported("started_phase");
        }
        let read_started = Instant::now();
        let current = self.issue(native_id)?;
        timing.remote_read = read_started.elapsed();
        let token_started = Instant::now();
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
        timing.token_check = token_started.elapsed();
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
        let write_started = Instant::now();
        self.request(
            "PUT",
            &self.endpoint(&format!("issue/{native_id}")),
            Some(&json!({"fields":fields})),
        )?;
        timing.remote_write = write_started.elapsed();
        let ack_started = Instant::now();
        let ticket = self.get(native_id)?;
        timing.acknowledgement = ack_started.elapsed();
        Ok((ticket, timing))
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

    /// The confidence rides on a `Confidence: NN%` comment paragraph (HS2-5YNASC).
    fn add_note_with_metadata(
        &self,
        native_id: &str,
        ctx: MutationContext,
        _: NoteKind,
        metadata: NoteMetadataInput,
        text: String,
    ) -> Result<ApiTicket, ProviderError> {
        let note_marker = note_trailer::note_marker(ctx.generated_id);
        if self
            .comments(native_id)?
            .iter()
            .any(|comment| adf_to_text(Some(&comment.body)).contains(&note_marker))
        {
            return self.get(native_id);
        }
        let body = note_trailer::compose_comment(&text, metadata.confidence, ctx.generated_id);
        self.request(
            "POST",
            &self.endpoint(&format!("issue/{native_id}/comment")),
            Some(&json!({ "body": text_to_adf(&body) })),
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
struct JiraChangelog {
    #[serde(default)]
    values: Vec<JiraHistory>,
    #[serde(rename = "isLast", default = "default_true")]
    is_last: bool,
}
fn default_true() -> bool {
    true
}
#[derive(Debug, Deserialize)]
struct JiraHistory {
    created: String,
    #[serde(default)]
    items: Vec<JiraHistoryItem>,
}
#[derive(Debug, Deserialize)]
struct JiraHistoryItem {
    field: String,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
}
#[derive(Debug, Deserialize)]
struct JiraStatusEntry {
    id: String,
    #[serde(rename = "statusCategory")]
    status_category: JiraStatusCategory,
}

/// Upper bound on changelog pages read for one detail request (100 entries each).
const MAX_HISTORY_PAGES: usize = 50;

#[derive(Debug, Deserialize)]
struct JiraComments {
    comments: Vec<JiraComment>,
    /// Total comments on the issue; absent from minimal responses.
    #[serde(default)]
    total: Option<usize>,
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
        attachment_edit: false,
        attachment_crop: false,
        assignment: true,
        review_requests: false,
        dependencies: false,
        up_next: false,
        close_reasons: false,
        claims: false,
        atomic_batch: false,
        not_working_report: false,
        // Comment trailers for scores are HS2-5YNASC; until then a score fails explicitly.
        note_confidence: true,
        ai_feedback: false,
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
        Err(ProviderError::InvalidNativeId {
            provider: "jira",
            id: value.into(),
        })
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

    /// HS2-5YNASC: the trailer survives Jira's ADF paragraph round trip, and an ordinary
    /// comment ending in the same words is never parsed as a score.
    #[test]
    fn note_confidence_round_trips_through_an_adf_comment_paragraph() {
        let note_id = hotsheet_model::Ulid::new();
        let score = hotsheet_model::Confidence::new(91).unwrap();
        let written = text_to_adf(&note_trailer::compose_comment(
            "## Result\nShipped.",
            Some(score),
            note_id,
        ));
        let mut done = issue("ENG-9", "done");
        done["fields"]["status"]["statusCategory"]["key"] = json!("done");
        let fake = Arc::new(Fake {
            responses: Mutex::new(
                vec![
                    response(200, json!({"comments":[]})),
                    response(201, json!({"id":"70"})),
                    response(200, done),
                    response(
                        200,
                        json!({"comments":[
                            {"id":"70","body":written,"created":"2026-08-26T00:02:00Z"},
                            {"id":"71","body":text_to_adf("Thanks.\n\nConfidence: 50%"),"created":"2026-08-26T00:03:00Z"}
                        ]}),
                    ),
                    // A scored done issue reads its changelog (HS2-N3RMTV); with no status
                    // transition the status catalogue is never requested.
                    response(200, json!({"values":[],"isLast":true})),
                ]
                .into(),
            ),
            ..Default::default()
        });
        let jira = provider(fake.clone());
        assert!(jira.supports_note_confidence());
        let ticket = jira
            .add_note_with_metadata(
                "ENG-9",
                MutationContext {
                    now: Timestamp::new("2026-08-26T00:02:00Z"),
                    generated_id: note_id,
                },
                NoteKind::Regular,
                NoteMetadataInput {
                    human_edited: false,
                    summary: None,
                    confidence: Some(score),
                    actor: None,
                    ai_feedback: None,
                },
                "## Result\nShipped.".into(),
            )
            .unwrap();
        let requests = fake.requests.lock().unwrap();
        assert_eq!(requests[1].0, "POST");
        assert_eq!(requests[1].3.as_ref().unwrap()["body"], written);
        assert_eq!(ticket.notes[0].text, "## Result\nShipped.");
        assert_eq!(ticket.notes[0].confidence, Some(91));
        assert_eq!(ticket.notes[1].confidence, None);
        assert_eq!(ticket.latest_confidence, Some(91));
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
    fn native_page_returns_jira_next_page_token_without_fetching_it() {
        let fake=Arc::new(Fake{responses:Mutex::new(vec![response(200,json!({"nextPageToken":"page-2","isLast":false,"issues":[issue("ENG-1","one")]}))].into()),..Default::default()});
        let page = provider(fake.clone())
            .query_page(&TicketQuery::default(), None, 100)
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.next_cursor.as_deref(), Some("page-2"));
        assert_eq!(fake.requests.lock().unwrap().len(), 1);
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
            disabled: false,
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

    /// HS2-7D9BPK: the reopen-history bound against a real Jira changelog and status
    /// catalogue. The Jira adapter cannot transition status, so the operator prepares an
    /// issue by hand: a Hot Sheet scored comment, Done, reopened, Done again (optionally a
    /// later scored comment). `HOTSHEET_JIRA_LIVE_REOPENED_EXPECTED` is the score the issue
    /// should report now, or `none`.
    #[test]
    #[ignore = "reads a prepared real Jira issue; set HOTSHEET_JIRA_LIVE_BASE_URL/PROJECT/EMAIL/TOKEN, HOTSHEET_JIRA_LIVE_REOPENED_ISSUE and HOTSHEET_JIRA_LIVE_REOPENED_EXPECTED"]
    fn jira_live_reopen_bounds_latest_confidence() {
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
            disabled: false,
        };
        let provider = JiraProvider::live(
            JiraConfig::from_connection(
                &connection,
                std::env::var("HOTSHEET_JIRA_LIVE_TOKEN").expect("live token"),
            )
            .unwrap(),
        );
        let issue = std::env::var("HOTSHEET_JIRA_LIVE_REOPENED_ISSUE").expect("prepared issue key");
        let expected =
            std::env::var("HOTSHEET_JIRA_LIVE_REOPENED_EXPECTED").expect("expected score");
        let expected = (expected != "none").then(|| expected.parse::<u8>().expect("0-100 or none"));
        let ticket = provider.get(&issue).unwrap();
        assert_eq!(
            ticket.status,
            Status::Completed,
            "prepare the issue in a done status"
        );
        assert!(
            ticket.notes.iter().any(|note| note.confidence.is_some()),
            "prepare the issue with a Hot Sheet scored comment"
        );
        assert_eq!(ticket.latest_confidence, expected);
    }

    fn scored_comment(id: &str, score: u64, created: &str) -> Value {
        let body = text_to_adf(&note_trailer::compose_comment(
            "Shipped.",
            Some(hotsheet_model::Confidence::new(score).unwrap()),
            hotsheet_model::Ulid::new(),
        ));
        json!({"id": id, "body": body, "created": created})
    }

    fn status_change(created: &str, from: &str, to: &str) -> Value {
        json!({"created": created, "items": [
            {"field": "assignee", "from": null, "to": "acct-1"},
            {"field": "status", "from": from, "to": to}
        ]})
    }

    /// HS2-N3RMTV: the changelog's transitions out of the `done` category bound the score.
    /// Two done statuses ("Done" 3, "Won't Do" 4) and paging by `startAt` are exercised; a
    /// move between done statuses is not a reopen.
    #[test]
    fn latest_confidence_is_bounded_by_the_latest_changelog_reopen() {
        let mut done = issue("ENG-9", "done");
        done["fields"]["status"]["statusCategory"]["key"] = json!("done");
        let changelog = || {
            vec![
                response(
                    200,
                    json!({"values":[
                        status_change("2026-08-26T08:30:00.000+0800", "1", "3")
                    ],"isLast":false}),
                ),
                response(
                    200,
                    json!({"values":[
                        // Reopened Done -> In Progress at 01:00Z (09:00 +0800).
                        status_change("2026-08-26T09:00:00.000+0800", "3", "2"),
                        status_change("2026-08-26T02:00:00.000+0000", "2", "3"),
                        // Done -> Won't Do stays done: not a reopen.
                        status_change("2026-08-26T03:00:00.000+0000", "3", "4")
                    ],"isLast":true}),
                ),
                response(
                    200,
                    json!([
                        {"id":"1","statusCategory":{"key":"new"}},
                        {"id":"2","statusCategory":{"key":"indeterminate"}},
                        {"id":"3","statusCategory":{"key":"done"}},
                        {"id":"4","statusCategory":{"key":"done"}}
                    ]),
                ),
            ]
        };
        let old = scored_comment("70", 80, "2026-08-26T00:45:00.000+0000");
        let mut responses = vec![
            response(200, done.clone()),
            response(200, json!({"comments":[old.clone()]})),
        ];
        responses.extend(changelog());
        responses.push(response(200, done));
        responses.push(response(
            200,
            json!({"comments":[old, scored_comment("71", 66, "2026-08-26T09:30:00.000+0800")]}),
        ));
        responses.extend(changelog());
        let fake = Arc::new(Fake {
            responses: Mutex::new(responses.into()),
            ..Default::default()
        });
        let jira = provider(fake.clone());
        assert_eq!(
            jira.get("ENG-9").unwrap().latest_confidence,
            None,
            "the pre-reopen score does not survive the re-completion"
        );
        assert_eq!(jira.get("ENG-9").unwrap().latest_confidence, Some(66));
        let requests = fake.requests.lock().unwrap();
        assert!(
            requests[2]
                .1
                .ends_with("/issue/ENG-9/changelog?startAt=0&maxResults=100")
        );
        assert!(
            requests[3]
                .1
                .ends_with("/issue/ENG-9/changelog?startAt=1&maxResults=100")
        );
        assert!(requests[4].1.ends_with("/rest/api/3/status"));
        assert_eq!(requests.len(), 10);
    }

    fn plain_comments(from: u64, count: u64) -> Vec<Value> {
        (from..from + count)
            .map(|id| json!({"id": id.to_string(), "body": text_to_adf(&format!("note {id}")), "created": "2026-08-26T00:00:00.000+0000"}))
            .collect()
    }

    /// HS2-9GS5TS: comments past the first 100 are read by paging `startAt` up to `total`, so
    /// the newest scored comment still sets `latest_confidence`.
    #[test]
    fn detail_read_pages_comments_by_start_at_up_to_total() {
        let mut done = issue("ENG-9", "done");
        done["fields"]["status"]["statusCategory"]["key"] = json!("done");
        let fake = Arc::new(Fake {
            responses: Mutex::new(
                vec![
                    response(200, done),
                    response(
                        200,
                        json!({"startAt":0,"maxResults":100,"total":101,"comments":plain_comments(1, 100)}),
                    ),
                    response(
                        200,
                        json!({"startAt":100,"maxResults":100,"total":101,"comments":[
                            scored_comment("101", 88, "2026-08-26T05:00:00.000+0000")
                        ]}),
                    ),
                    response(200, json!({"values":[],"isLast":true})),
                ]
                .into(),
            ),
            ..Default::default()
        });
        let ticket = provider(fake.clone()).get("ENG-9").unwrap();
        assert_eq!(ticket.notes.len(), 101);
        assert_eq!(ticket.latest_confidence, Some(88));
        let requests = fake.requests.lock().unwrap();
        assert!(
            requests[1]
                .1
                .ends_with("/issue/ENG-9/comment?startAt=0&maxResults=100")
        );
        assert!(
            requests[2]
                .1
                .ends_with("/issue/ENG-9/comment?startAt=100&maxResults=100")
        );
        assert_eq!(requests.len(), 4);
    }

    /// Page boundaries: `total` exactly 100 needs no second request; without `total`, a full
    /// page asks once more and an empty page ends the list.
    #[test]
    fn comment_paging_stops_at_total_or_an_empty_page() {
        let fake = Arc::new(Fake {
            responses: Mutex::new(
                vec![
                    response(200, issue("ENG-9", "exact")),
                    response(200, json!({"total":100,"comments":plain_comments(1, 100)})),
                    response(200, issue("ENG-9", "no total")),
                    response(200, json!({"comments":plain_comments(1, 100)})),
                    response(200, json!({"comments":[]})),
                ]
                .into(),
            ),
            ..Default::default()
        });
        let jira = provider(fake.clone());
        assert_eq!(jira.get("ENG-9").unwrap().notes.len(), 100);
        assert_eq!(jira.get("ENG-9").unwrap().notes.len(), 100);
        let requests = fake.requests.lock().unwrap();
        assert!(
            requests[4]
                .1
                .ends_with("/issue/ENG-9/comment?startAt=100&maxResults=100")
        );
        assert_eq!(requests.len(), 5);
    }

    /// The `add_note` retry finds its idempotency marker on the second page and posts nothing.
    #[test]
    fn add_note_retry_finds_its_marker_past_the_first_comment_page() {
        let note_id = hotsheet_model::Ulid::new();
        let marker = json!({
            "id": "101",
            "body": text_to_adf(&format!("Done.\n\n{}", note_trailer::note_marker(note_id))),
            "created": "2026-08-26T05:00:00.000+0000"
        });
        let first = || response(200, json!({"total":101,"comments":plain_comments(1, 100)}));
        let fake = Arc::new(Fake {
            responses: Mutex::new(
                vec![
                    first(),
                    response(200, json!({"total":101,"comments":[marker.clone()]})),
                    response(200, issue("ENG-9", "retried")),
                    first(),
                    response(200, json!({"total":101,"comments":[marker]})),
                ]
                .into(),
            ),
            ..Default::default()
        });
        let ticket = provider(fake.clone())
            .add_note(
                "ENG-9",
                MutationContext {
                    now: Timestamp::new("2026-08-27T00:00:00Z"),
                    generated_id: note_id,
                },
                NoteKind::Regular,
                "Done.".into(),
            )
            .unwrap();
        assert_eq!(ticket.notes.len(), 101);
        let requests = fake.requests.lock().unwrap();
        assert!(
            requests.iter().all(|request| request.0 == "GET"),
            "no duplicate POST"
        );
        assert_eq!(requests.len(), 5);
    }
    #[test]
    fn unscored_or_open_issues_skip_the_changelog_request() {
        let mut done = issue("ENG-9", "done");
        done["fields"]["status"]["statusCategory"]["key"] = json!("done");
        let fake = Arc::new(Fake {
            responses: Mutex::new(
                vec![
                    response(200, issue("ENG-9", "open")),
                    response(
                        200,
                        json!({"comments":[scored_comment("70", 80, "2026-08-26T00:45:00.000+0000")]}),
                    ),
                    response(200, done),
                    response(200, json!({"comments":[]})),
                ]
                .into(),
            ),
            ..Default::default()
        });
        let jira = provider(fake.clone());
        assert_eq!(jira.get("ENG-9").unwrap().latest_confidence, None);
        assert_eq!(jira.get("ENG-9").unwrap().latest_confidence, None);
        assert_eq!(fake.requests.lock().unwrap().len(), 4);
    }
}
