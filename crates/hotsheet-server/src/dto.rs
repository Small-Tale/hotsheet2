//! Request / response DTOs.

use crate::*;

// ---- request / response DTOs -----------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ListParams {
    /// Checkout page only: read local git sources without touching external credentials.
    pub(crate) source: Option<String>,
    pub(crate) status: Option<String>,
    /// Built-in multi-status client collection (`queue`, `archive`, or `trash`).
    pub(crate) collection: Option<String>,
    pub(crate) priority: Option<String>,
    pub(crate) category: Option<String>,
    /// Comma-separated; a ticket must carry all of them.
    pub(crate) tags: Option<String>,
    pub(crate) text: Option<String>,
    pub(crate) up_next: Option<bool>,
    pub(crate) open: Option<bool>,
    /// Filter by close reason (completed|not_planned|duplicate|obsolete|works_as_designed).
    pub(crate) close_reason: Option<String>,
    /// `true` = only closed tickets; `false` = only tickets with no close reason.
    pub(crate) closed: Option<bool>,
    /// Only tickets assigned to this person (git email).
    pub(crate) assignee: Option<String>,
    /// Only tickets with a review request for this person (git email).
    pub(crate) review_requested: Option<String>,
    /// Only tickets whose review was requested by this person (git email).
    pub(crate) review_by: Option<String>,
    /// `true` = only claimed tickets; `false` = only unclaimed.
    pub(crate) claimed: Option<bool>,
    /// `true` = only blocked tickets; `false` = only unblocked (HS2-T84F9F).
    pub(crate) blocked: Option<bool>,
    /// ISO-8601 `created_at` / `updated_at` range bounds (inclusive).
    pub(crate) created_after: Option<String>,
    pub(crate) created_before: Option<String>,
    pub(crate) updated_after: Option<String>,
    pub(crate) updated_before: Option<String>,
    pub(crate) completed_after: Option<String>,
    pub(crate) completed_before: Option<String>,
    pub(crate) verified_after: Option<String>,
    pub(crate) verified_before: Option<String>,
    /// Inclusive bounds on the derived `latest_confidence` (0-100, HS2-RD4M29).
    pub(crate) min_confidence: Option<u8>,
    pub(crate) max_confidence: Option<u8>,
    pub(crate) has_attachment: Option<bool>,
    pub(crate) has_media_annotation: Option<bool>,
    /// Checkout-only filter: whether repository commits reference the ticket slug.
    pub(crate) has_commit: Option<bool>,
    /// Comma-separated attachment filename patterns; `*` is a wildcard.
    pub(crate) attachment: Option<String>,
    pub(crate) sort: Option<String>,
    /// Sort direction for bounded checkout pages (`ascending` or `descending`).
    pub(crate) direction: Option<String>,
    pub(crate) limit: Option<usize>,
    /// Opt into the bounded checkout page envelope. Capped to protect server and browser.
    pub(crate) page_size: Option<usize>,
    /// Opaque checkout-level cursor returned by a prior paged response.
    pub(crate) cursor: Option<String>,
    /// Eight comma-separated RFC 3339 local-day boundaries for exact seven-day summaries.
    pub(crate) summary_days: Option<String>,
    /// Checkout pages only: `counts=false` skips the navigation counts (returned as `null`),
    /// so whole-checkout walkers avoid a provider summary walk per page (HS2-VPEAM4).
    pub(crate) counts: Option<bool>,
    /// Keyset cursor (a ULID): return rows strictly after this one in `sort` order (HS2-TCDTCH).
    pub(crate) page_after: Option<String>,
    /// Omit the Markdown body from each row (default true). `compact=false` keeps it.
    pub(crate) compact: Option<bool>,
    /// Comma-separated field allow-list for a leaner-than-compact projection (HS2-GY3GWT):
    /// each row keeps only these keys (plus `slug`). Empty/absent = the full compact row.
    pub(crate) fields: Option<String>,
}

/// Parse the `fields=` allow-list (comma-separated, empties dropped).
pub(crate) fn parse_fields(fields: &Option<String>) -> Vec<String> {
    fields
        .as_deref()
        .map(|f| {
            f.split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Serialize compact rows to a JSON array, applying the `fields` projection if any.
pub(crate) fn rows_to_json(rows: Vec<TicketRow>, fields: &[String]) -> serde_json::Value {
    let mut vals: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| serde_json::to_value(r).unwrap_or(serde_json::Value::Null))
        .collect();
    hotsheet_ticketing::wire::project_fields(&mut vals, fields);
    serde_json::Value::Array(vals)
}

impl ListParams {
    /// Build the `TicketQuery`, resolving the `me` sentinel in person filters against the
    /// store's git identity (HS2-TCDTCH). `store_root` is the store the query runs against.
    pub(crate) fn into_query(self, store_root: &FsPath) -> Result<TicketQuery, ApiError> {
        let sort = match self.sort {
            Some(s) => s
                .parse::<SortKey>()
                .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e))?,
            None => SortKey::default(),
        };
        let descending = match self.direction.as_deref() {
            None | Some("ascending") => false,
            Some("descending") => true,
            Some(other) => {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    format!("invalid direction '{other}'"),
                ));
            }
        };
        // `me` → the store's git user.email; a `me` that can't be resolved is an error, not a
        // silent match-everyone (docs/10 §10.3).
        let resolve_person = |v: Option<String>| -> Result<Option<String>, ApiError> {
            match v {
                None => Ok(None),
                Some(raw) if raw.eq_ignore_ascii_case(hotsheet_ticketing::ME) => {
                    hotsheet_ticketing::current_user_email(store_root)
                        .map(Some)
                        .ok_or_else(|| {
                            ApiError::new(
                                StatusCode::BAD_REQUEST,
                                "cannot resolve 'me': no git user.email configured",
                            )
                        })
                }
                Some(raw) => Ok(Some(raw)),
            }
        };
        let page_after = match self.page_after {
            Some(s) => Some(Ulid::from_string(&s).map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid page_after cursor (not a ULID)",
                )
            })?),
            None => None,
        };
        for (name, bound) in [
            ("min_confidence", self.min_confidence),
            ("max_confidence", self.max_confidence),
        ] {
            if bound.is_some_and(|value| value > Confidence::MAX) {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    format!("{name} must be an integer from 0 to 100"),
                ));
            }
        }
        Ok(TicketQuery {
            status: opt_parse(self.status.as_deref())?,
            collection: opt_parse(self.collection.as_deref())?,
            priority: opt_parse(self.priority.as_deref())?,
            category: self.category,
            tags: self
                .tags
                .map(|t| {
                    t.split(',')
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            text: self.text,
            up_next_only: self.up_next.unwrap_or(false),
            open_only: self.open.unwrap_or(false),
            close_reason: opt_parse(self.close_reason.as_deref())?,
            closed: self.closed,
            assignee: resolve_person(self.assignee)?,
            review_requested: resolve_person(self.review_requested)?,
            review_by: resolve_person(self.review_by)?,
            claimed: self.claimed,
            blocked: self.blocked,
            created_after: self.created_after,
            created_before: self.created_before,
            updated_after: self.updated_after,
            updated_before: self.updated_before,
            completed_after: self.completed_after,
            completed_before: self.completed_before,
            verified_after: self.verified_after,
            verified_before: self.verified_before,
            min_confidence: self.min_confidence,
            max_confidence: self.max_confidence,
            has_attachment: self.has_attachment,
            has_media_annotation: self.has_media_annotation,
            attachment_patterns: self
                .attachment
                .map(|values| {
                    values
                        .split(',')
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            sort,
            descending,
            limit: self.limit,
            page_after,
            after_key: None,
        })
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateReq {
    pub(crate) title: String,
    pub(crate) category: Option<String>,
    pub(crate) priority: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) details: Option<String>,
    pub(crate) tags: Option<Vec<String>>,
    pub(crate) up_next: Option<bool>,
    /// Blocker tickets (slug or ULID), resolved to ULIDs on create.
    pub(crate) blocked_by: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct UpdateReq {
    pub(crate) expected_token: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) details: Option<String>,
    pub(crate) category: Option<String>,
    pub(crate) priority: Option<String>,
    pub(crate) status: Option<String>,
    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    pub(crate) started_phase: Option<Option<String>>,
    pub(crate) tags: Option<Vec<String>>,
    pub(crate) up_next: Option<bool>,
    /// Replace the blocker set (slug or ULID); `[]` clears it, absent leaves it.
    pub(crate) blocked_by: Option<Vec<String>>,
    /// Set, clear with JSON null, or leave unchanged when absent.
    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    pub(crate) blocked_reason: Option<Option<String>>,
    /// Optional note to append alongside the field update.
    pub(crate) note: Option<String>,
    /// Rating revision. This is a feedback-only update; repeated ratings from the
    /// same actor and target retain one source note id.
    pub(crate) ai_feedback: Option<AiFeedbackReq>,
    /// Existing note ULID to edit; absent appends a new note.
    pub(crate) note_id: Option<String>,
    /// Kind of the appended note; defaults to regular for older clients.
    pub(crate) note_kind: Option<NoteKind>,
    /// Optional concise plain-text headline used by timeline presentations.
    pub(crate) note_summary: Option<String>,
    /// Optional AI completion confidence (integer 0-100) on the appended note
    /// (HS2-DWTJ43), or on the note named by `note_id`, where JSON null clears it
    /// (HS2-CY4CWC). Kept as raw JSON so a malformed value gets an explicit 400.
    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    pub(crate) note_confidence: Option<Option<serde_json::Value>>,
    /// Who is acting (HS2-RD4M29): `{"role":"human|ai|system","id":"..."}`. Absent is
    /// unspecified, never assumed AI.
    #[serde(default)]
    pub(crate) actor: Option<ActorReq>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AiFeedbackReq {
    pub(crate) target: String,
    /// Required; JSON null explicitly withdraws the current rating.
    pub(crate) rating: serde_json::Value,
    pub(crate) explanation: Option<String>,
}

impl UpdateReq {
    pub(crate) fn validate_feedback_only(&self) -> Result<(), ApiError> {
        if self.title.is_some()
            || self.details.is_some()
            || self.category.is_some()
            || self.priority.is_some()
            || self.status.is_some()
            || self.started_phase.is_some()
            || self.tags.is_some()
            || self.up_next.is_some()
            || self.blocked_by.is_some()
            || self.blocked_reason.is_some()
            || self.note.is_some()
            || self.note_id.is_some()
            || self.note_kind.is_some()
            || self.note_summary.is_some()
            || self.note_confidence.is_some()
        {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "AI feedback must be updated separately from ticket fields and notes",
            ));
        }
        Ok(())
    }

    /// Validate the optional note confidence as a JSON integer from 0 to 100.
    ///
    /// Appending: a score needs a non-empty note, and null means "no score".
    /// Editing (`note_id`): absent leaves the score unchanged, an integer replaces it,
    /// and null clears it; the note text may be omitted.
    pub(crate) fn note_confidence_change(
        &self,
        editing_note: bool,
    ) -> Result<Option<Option<Confidence>>, ApiError> {
        let Some(value) = self.note_confidence.as_ref() else {
            return Ok(None);
        };
        let Some(value) = value else {
            return Ok(editing_note.then_some(None));
        };
        if !editing_note && self.note.as_deref().is_none_or(str::is_empty) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "note_confidence requires a non-empty note",
            ));
        }
        value
            .as_u64()
            .ok_or_else(|| ConfidenceError(value.to_string()))
            .and_then(Confidence::new)
            .map(|confidence| Some(Some(confidence)))
            .map_err(|error| {
                ApiError::new(StatusCode::BAD_REQUEST, format!("note_confidence: {error}"))
            })
    }
}

pub(crate) fn deserialize_nullable_patch<'de, D, T>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// `POST /batch` body: apply the same field update to every listed ticket (HS2-86).
#[derive(Debug, Deserialize)]
pub(crate) struct BatchReq {
    /// Tickets to update (slug or ULID).
    pub(crate) ids: Vec<String>,
    /// The same update applied to each — the `UpdateReq` fields, flattened in.
    #[serde(flatten)]
    pub(crate) update: UpdateReq,
}

/// The per-ticket outcome of a batch update.
#[derive(Debug, Serialize)]
pub(crate) struct BatchResult {
    /// Slugs of the tickets updated.
    pub(crate) updated: Vec<String>,
    /// Tickets that failed, with why (a bad batch never aborts the rest).
    pub(crate) errors: Vec<BatchError>,
}

#[derive(Debug, Serialize)]
pub(crate) struct BatchError {
    pub(crate) id: String,
    pub(crate) message: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CloseReq {
    pub(crate) reason: String,
    pub(crate) duplicate_of: Option<DuplicateOfReq>,
    /// Who is closing (HS2-RD4M29); an `ai` completed-close needs a scored cycle.
    #[serde(default)]
    pub(crate) actor: Option<ActorReq>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum DuplicateOfReq {
    Legacy(String),
    Qualified(ProjectTicketRef),
}

#[derive(Debug, Deserialize)]
pub(crate) struct ClaimReq {
    pub(crate) worker: Option<String>,
    pub(crate) label: Option<String>,
    pub(crate) lease_minutes: Option<i64>,
    /// Estimated completion time: a duration such as `45m` or an RFC 3339 timestamp (HS2-DQQ0AX).
    pub(crate) eta: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReleaseReq {
    pub(crate) worker: Option<String>,
    pub(crate) force: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RenewReq {
    pub(crate) worker: Option<String>,
    pub(crate) lease_minutes: Option<i64>,
    /// A new estimated completion time; omitted keeps the current one (HS2-DQQ0AX).
    pub(crate) eta: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WsParams {
    pub(crate) secret: Option<String>,
    /// The checkout this subscription serves and the client (browser tab) holding it, so the
    /// server knows which projects are open (HS2-ARJ9J1).
    pub(crate) checkout: Option<String>,
    pub(crate) client: Option<String>,
}

// The full-ticket + note wire DTOs (`ApiTicket`/`ApiNote`) and their `From<&Ticket>`
// mapping live in `hotsheet_ticketing::wire` and are re-exported at the top of this
// module — one definition, shared with the MCP shim (wire SSOT, `docs/04` §4.2).
