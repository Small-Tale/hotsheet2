//! Source-agnostic bounded paging across every source of one checkout (HS2-JVF20F).
//!
//! A checkout can span several git stores and hosted-provider connections. This module
//! owns the batched value-keyset k-way merge, the v2 continuation-cursor codec, and the
//! filter fingerprint that binds a cursor to its effective filter set, so the server
//! (over its SQLite indexes and provider adapters) and the serverless MCP backend (over
//! file scans) return the same `{items, next_cursor, counts}` page and accept each other's
//! cursors whenever their source sets match.
//!
//! Each caller supplies one fetch closure: given a source index, the last key already
//! fetched from that source, its provider resume hint, and a batch size, it returns a
//! bounded batch of rows strictly after that key in [`checkout_order::compare`] order.

use crate::checkout_order::{self, MergeKey};
use crate::provider::ProviderTicketSummary;
use crate::{SortKey, TicketQuery};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// The current checkout cursor format. Row-identity `v1` cursors predate value keysets and
/// are rejected as stale.
pub const CURSOR_VERSION: u8 = 2;

/// Default page size when a caller asks for a page without naming one.
pub const DEFAULT_PAGE_SIZE: usize = 200;

/// Why a checkout page could not be produced.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CheckoutPageError {
    /// The cursor was computed for another format, source set, sort, direction, or filters.
    #[error("stale checkout ticket cursor")]
    StaleCursor,
    /// The cursor is not a checkout cursor at all.
    #[error("invalid checkout ticket cursor")]
    InvalidCursor,
    /// A source reported more rows but returned none, which would loop forever.
    #[error("provider returned a non-advancing keyset page")]
    NonAdvancing,
    /// `summary_days` was not eight ascending RFC 3339 boundaries.
    #[error("summary_days must contain eight ascending RFC 3339 day boundaries")]
    InvalidSummaryDays,
    /// `counts` was neither `true` nor `false`.
    #[error("counts must be true or false")]
    InvalidCounts,
    /// The clock or cursor could not be serialized.
    #[error("{0}")]
    Internal(String),
}

/// One source's continuation state. Positions are value keysets (HS2-74H84S): every source
/// resumes strictly after the cursor's `last` key, so `resume` is only a provider-owned hint
/// positioned at or before that key and `exhausted` skips sources that had no further rows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct SourceCursor {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<String>,
    #[serde(default)]
    pub exhausted: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct MergeCursor {
    version: u8,
    sort: String,
    descending: bool,
    /// Canonical fingerprint of the effective filter set the positions were computed for.
    #[serde(default)]
    filters: String,
    /// Sort-key values of the last emitted row (the global value keyset).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last: Option<MergeKey>,
    sources: Vec<SourceCursor>,
}

/// The cursor source key of a git store: `git:{connection id}`.
#[must_use]
pub fn git_source_key(connection_id: &str) -> String {
    format!("git:{connection_id}")
}

/// The cursor source key of a hosted-provider connection: `provider:{connection id}`.
#[must_use]
pub fn provider_source_key(connection_id: &str) -> String {
    format!("provider:{connection_id}")
}

#[must_use]
pub fn sort_name(sort: SortKey) -> &'static str {
    match sort {
        SortKey::Id => "id",
        SortKey::Created => "created",
        SortKey::Updated => "updated",
        SortKey::Priority => "priority",
        SortKey::Status => "status",
        SortKey::Title => "title",
    }
}

/// Canonical fingerprint of a checkout page's effective filters (HS2-Z1TQ7Z).
///
/// Ordering fields (`sort`, `descending`) are bound separately, and paging fields
/// (`limit`, `page_after`, `after_key`) are excluded. Set-valued filters are sorted and
/// de-duplicated so equivalent query strings in a different parameter order share a
/// fingerprint.
#[must_use]
pub fn filter_fingerprint(query: &TicketQuery, has_commit: Option<bool>) -> String {
    use sha2::{Digest, Sha256};
    let mut canonical = query.clone();
    canonical.sort = SortKey::default();
    canonical.descending = false;
    canonical.limit = None;
    canonical.page_after = None;
    canonical.after_key = None;
    canonical.tags.sort();
    canonical.tags.dedup();
    canonical.attachment_patterns.sort();
    canonical.attachment_patterns.dedup();
    let text = format!("{canonical:?}|has_commit={has_commit:?}");
    format!("{:x}", Sha256::digest(text.as_bytes()))[..32].to_string()
}

/// Encode a continuation cursor as `v2.<hex JSON>`.
///
/// # Errors
/// [`CheckoutPageError::Internal`] only if JSON serialization fails.
pub fn encode_cursor(
    sources: &[SourceCursor],
    sort: SortKey,
    descending: bool,
    filters: &str,
    last: Option<&MergeKey>,
) -> Result<String, CheckoutPageError> {
    let bytes = serde_json::to_vec(&MergeCursor {
        version: CURSOR_VERSION,
        sort: sort_name(sort).into(),
        descending,
        filters: filters.into(),
        last: last.cloned(),
        sources: sources.to_vec(),
    })
    .map_err(|error| CheckoutPageError::Internal(error.to_string()))?;
    Ok(format!(
        "v{CURSOR_VERSION}.{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

/// Decode a checkout cursor into the last emitted key and every source's state. A cursor
/// from an older format, or one computed for another source set, sort, direction, or
/// filter set, is rejected as stale rather than silently reinterpreted.
///
/// # Errors
/// [`CheckoutPageError::StaleCursor`] or [`CheckoutPageError::InvalidCursor`].
pub fn decode_cursor(
    value: Option<&str>,
    source_keys: &[String],
    sort: SortKey,
    descending: bool,
    filters: &str,
) -> Result<(Option<MergeKey>, Vec<SourceCursor>), CheckoutPageError> {
    let Some(value) = value else {
        return Ok((
            None,
            source_keys
                .iter()
                .map(|key| SourceCursor {
                    key: key.clone(),
                    ..Default::default()
                })
                .collect(),
        ));
    };
    if value.starts_with("v1.") {
        return Err(CheckoutPageError::StaleCursor);
    }
    let hex = value
        .strip_prefix(&format!("v{CURSOR_VERSION}."))
        .ok_or(CheckoutPageError::InvalidCursor)?;
    if hex.len() % 2 != 0 || !hex.is_ascii() {
        return Err(CheckoutPageError::InvalidCursor);
    }
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CheckoutPageError::InvalidCursor)?;
    let cursor = serde_json::from_slice::<MergeCursor>(&bytes)
        .map_err(|_| CheckoutPageError::InvalidCursor)?;
    if cursor.version != CURSOR_VERSION
        || cursor.sort != sort_name(sort)
        || cursor.descending != descending
        || cursor.filters != filters
        || cursor.sources.len() != source_keys.len()
        || cursor
            .sources
            .iter()
            .zip(source_keys)
            .any(|(source, key)| &source.key != key)
    {
        return Err(CheckoutPageError::StaleCursor);
    }
    Ok((cursor.last, cursor.sources))
}

/// One fetched source row. `value` is `None` when a post-filter removed the row: it still
/// advances the source past `key`, but is never emitted.
#[derive(Debug, Clone)]
pub struct SourceRow {
    pub key: MergeKey,
    pub resume: Option<String>,
    pub value: Option<Value>,
}

/// One bounded source read, in checkout order, strictly after the requested key.
/// `exhausted` means no matching row exists after the last returned row.
#[derive(Debug, Clone, Default)]
pub struct SourceBatch {
    pub rows: Vec<SourceRow>,
    pub exhausted: bool,
}

/// One fetch request passed to a source's closure.
#[derive(Debug, Clone, Copy)]
pub struct SourceFetch<'a> {
    /// Index into the `source_keys` passed to [`merge_page`].
    pub source: usize,
    /// Return only rows strictly after this key (`None` = from the top).
    pub after: Option<&'a MergeKey>,
    /// The provider resume hint of the last row fetched from this source.
    pub resume: Option<&'a str>,
    /// The batch size wanted; fewer rows are allowed only when the source is exhausted.
    pub want: usize,
}

/// One merged page: at most `page_size` rows in the global order, plus the continuation
/// cursor when some source still has a row after the last one emitted.
#[derive(Debug, Clone, Default)]
pub struct MergedPage {
    pub items: Vec<Value>,
    pub next_cursor: Option<String>,
}

struct Buffered {
    value: Value,
    key: MergeKey,
    resume: Option<String>,
}

struct SourceState {
    cursor: SourceCursor,
    fetch_after: Option<MergeKey>,
    fetch_resume: Option<String>,
    buffer: VecDeque<Buffered>,
}

/// Merge one bounded page across every checkout source (HS2-74H84S, HS2-BGZ0NY).
///
/// Each source is read in bounded batches strictly after the last key it fetched, so a page
/// costs at most a few source reads regardless of the checkout's size, and editing or
/// purging the last emitted row can neither end nor rewind a traversal.
///
/// # Errors
/// Cursor errors, a non-advancing source, or any error the fetch closure returns.
pub fn merge_page<E: From<CheckoutPageError>>(
    source_keys: &[String],
    cursor: Option<&str>,
    sort: SortKey,
    descending: bool,
    filters: &str,
    page_size: usize,
    mut fetch: impl FnMut(SourceFetch<'_>) -> Result<SourceBatch, E>,
) -> Result<MergedPage, E> {
    let (mut last, source_cursors) = decode_cursor(cursor, source_keys, sort, descending, filters)?;
    let mut fill = |index: usize, state: &mut SourceState, want: usize| -> Result<(), E> {
        while state.buffer.is_empty() && !state.cursor.exhausted {
            let batch = fetch(SourceFetch {
                source: index,
                after: state.fetch_after.as_ref(),
                resume: state.fetch_resume.as_deref(),
                want,
            })?;
            if batch.rows.is_empty() && !batch.exhausted {
                return Err(CheckoutPageError::NonAdvancing.into());
            }
            state.cursor.exhausted = batch.exhausted;
            for row in batch.rows {
                state.fetch_after = Some(row.key.clone());
                state.fetch_resume.clone_from(&row.resume);
                if let Some(value) = row.value {
                    state.buffer.push_back(Buffered {
                        value,
                        key: row.key,
                        resume: row.resume,
                    });
                }
            }
        }
        Ok(())
    };
    let mut sources = source_cursors
        .into_iter()
        .map(|cursor| SourceState {
            fetch_after: last.clone(),
            fetch_resume: cursor.resume.clone(),
            cursor,
            buffer: VecDeque::new(),
        })
        .collect::<Vec<_>>();
    for (index, source) in sources.iter_mut().enumerate() {
        fill(index, source, page_size)?;
    }
    let mut items = Vec::with_capacity(page_size);
    while items.len() < page_size {
        let Some(index) = sources
            .iter()
            .enumerate()
            .filter_map(|(index, source)| source.buffer.front().map(|item| (index, item)))
            .min_by(|(_, left), (_, right)| {
                checkout_order::compare(&left.key, &right.key, sort, descending)
            })
            .map(|(index, _)| index)
        else {
            break;
        };
        let source = &mut sources[index];
        let Some(item) = source.buffer.pop_front() else {
            break;
        };
        source.cursor.resume = item.resume;
        last = Some(item.key);
        items.push(item.value);
        if source.buffer.is_empty() && items.len() < page_size {
            fill(index, source, page_size - items.len())?;
        }
    }
    // A continuation exists only when some source still has a row after `last`.
    for (index, source) in sources.iter_mut().enumerate() {
        fill(index, source, 1)?;
    }
    let next_cursor = if sources.iter().any(|source| !source.buffer.is_empty()) {
        let cursors = sources
            .iter()
            .map(|source| SourceCursor {
                exhausted: source.buffer.is_empty(),
                ..source.cursor.clone()
            })
            .collect::<Vec<_>>();
        Some(encode_cursor(
            &cursors,
            sort,
            descending,
            filters,
            last.as_ref(),
        )?)
    } else {
        None
    };
    Ok(MergedPage { items, next_cursor })
}

/// Checkout-wide navigation counts: the sum of every source's summary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckoutTicketCounts {
    pub total: u64,
    pub queued: u64,
    pub backlog: u64,
    pub archive: u64,
    pub trash: u64,
    pub open: u64,
    pub up_next: u64,
    pub active: u64,
    pub started: u64,
    pub verified: u64,
    pub completed_today: u64,
    pub completion_trend: Vec<u64>,
}

impl CheckoutTicketCounts {
    /// Empty counts whose completion trend already has one zero bucket per day window, so
    /// every source kind (index or provider walk) reports the same trend length.
    #[must_use]
    pub fn for_days(day_starts: &[String]) -> Self {
        Self {
            completion_trend: vec![0; day_starts.len().saturating_sub(1)],
            ..Self::default()
        }
    }

    pub fn add(&mut self, summary: ProviderTicketSummary) {
        self.total += summary.total;
        self.queued += summary.queued;
        self.backlog += summary.backlog;
        self.archive += summary.archive;
        self.trash += summary.trash;
        self.open += summary.open;
        self.up_next += summary.up_next;
        self.active += summary.active;
        self.started += summary.started;
        self.verified += summary.verified;
        self.completed_today += summary.completed_today;
        if self.completion_trend.len() < summary.completion_trend.len() {
            self.completion_trend
                .resize(summary.completion_trend.len(), 0);
        }
        for (index, count) in summary.completion_trend.into_iter().enumerate() {
            self.completion_trend[index] += count;
        }
    }
}

/// The paged checkout envelope shared by the server and the serverless MCP backend.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CheckoutTicketPage {
    pub items: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Navigation counts, or an explicit `null` when the caller passed `counts=false`
    /// (HS2-VPEAM4): walkers that ignore counts skip a full summary read per page.
    pub counts: Option<CheckoutTicketCounts>,
}

/// Parse the `counts` page parameter: absent or `true` computes counts, `false` omits them.
///
/// # Errors
/// [`CheckoutPageError::InvalidCounts`] for any other value.
pub fn wants_counts(value: Option<&str>) -> Result<bool, CheckoutPageError> {
    match value {
        None | Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(_) => Err(CheckoutPageError::InvalidCounts),
    }
}

/// The eight day boundaries (seven local days plus tomorrow's start) the completion trend
/// buckets use: the caller's `summary_days`, or the last seven UTC days.
///
/// # Errors
/// [`CheckoutPageError::InvalidSummaryDays`] for a malformed `summary_days`.
pub fn completion_day_starts(
    value: Option<&str>,
    now: OffsetDateTime,
) -> Result<Vec<String>, CheckoutPageError> {
    if let Some(value) = value {
        let starts = value.split(',').map(str::to_owned).collect::<Vec<_>>();
        if starts.len() != 8
            || starts
                .iter()
                .any(|start| OffsetDateTime::parse(start, &Rfc3339).is_err())
            || starts.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(CheckoutPageError::InvalidSummaryDays);
        }
        return Ok(starts);
    }
    let today = now.replace_time(time::Time::MIDNIGHT);
    (0..=7)
        .map(|index| {
            (today - time::Duration::days(6 - index))
                .format(&Rfc3339)
                .map_err(|error| CheckoutPageError::Internal(error.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(source: &str, id: &str) -> MergeKey {
        MergeKey {
            native_id: id.into(),
            qualified_id: format!("{source}:{id}"),
            title: id.into(),
            updated_at: "2026-09-01T00:00:00Z".into(),
            ..Default::default()
        }
    }

    /// An in-memory source: rows sorted by id, some marked filtered.
    struct Fake {
        source: &'static str,
        ids: Vec<(&'static str, bool)>,
    }

    impl Fake {
        fn fetch(&self, request: SourceFetch<'_>, calls: &mut usize) -> SourceBatch {
            *calls += 1;
            let rows = self
                .ids
                .iter()
                .map(|(id, keep)| (key(self.source, id), *keep))
                .filter(|(key, _)| {
                    request.after.is_none_or(|after| {
                        checkout_order::compare(key, after, SortKey::Id, false).is_gt()
                    })
                })
                .collect::<Vec<_>>();
            let exhausted = rows.len() <= request.want;
            SourceBatch {
                rows: rows
                    .into_iter()
                    .take(request.want)
                    .map(|(key, keep)| SourceRow {
                        value: keep.then(|| json!({ "id": key.qualified_id.clone() })),
                        resume: Some(format!("hint-{}", key.native_id)),
                        key,
                    })
                    .collect(),
                exhausted,
            }
        }
    }

    fn walk(sources: &[Fake], page_size: usize) -> (Vec<Vec<String>>, usize) {
        let keys = sources
            .iter()
            .map(|source| git_source_key(source.source))
            .collect::<Vec<_>>();
        let mut cursor = None;
        let mut pages = Vec::new();
        let mut calls = 0;
        loop {
            let page = merge_page::<CheckoutPageError>(
                &keys,
                cursor.as_deref(),
                SortKey::Id,
                false,
                "f",
                page_size,
                |request| Ok(sources[request.source].fetch(request, &mut calls)),
            )
            .unwrap();
            pages.push(
                page.items
                    .iter()
                    .map(|item| item["id"].as_str().unwrap().to_owned())
                    .collect(),
            );
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        (pages, calls)
    }

    #[test]
    fn merge_page_interleaves_sources_and_resumes_across_pages() {
        let sources = [
            Fake {
                source: "a",
                ids: vec![("1", true), ("3", true), ("5", true)],
            },
            Fake {
                source: "b",
                ids: vec![("2", true), ("4", true)],
            },
        ];
        let (pages, _) = walk(&sources, 2);
        assert_eq!(pages, [vec!["a:1", "b:2"], vec!["a:3", "b:4"], vec!["a:5"]]);
    }

    #[test]
    fn filtered_rows_advance_the_source_without_being_emitted() {
        let sources = [Fake {
            source: "a",
            ids: vec![("1", false), ("2", false), ("3", true), ("4", false)],
        }];
        let (pages, _) = walk(&sources, 1);
        assert_eq!(
            pages,
            [vec!["a:3"]],
            "no empty trailing page for filtered tail"
        );
    }

    #[test]
    fn empty_sources_yield_one_empty_page() {
        let (pages, _) = walk(&[], 5);
        assert_eq!(pages, [Vec::<String>::new()]);
        let (pages, _) = walk(
            &[Fake {
                source: "a",
                ids: vec![],
            }],
            5,
        );
        assert_eq!(pages, [Vec::<String>::new()]);
    }

    #[test]
    fn a_non_advancing_source_fails_instead_of_looping() {
        let error = merge_page::<CheckoutPageError>(
            &[git_source_key("a")],
            None,
            SortKey::Id,
            false,
            "f",
            3,
            |_| Ok(SourceBatch::default()),
        )
        .unwrap_err();
        assert_eq!(error, CheckoutPageError::NonAdvancing);
    }

    #[test]
    fn exhausted_sources_are_not_refetched_on_later_pages() {
        let sources = [
            Fake {
                source: "a",
                ids: vec![("1", true)],
            },
            Fake {
                source: "b",
                ids: (2..40)
                    .map(|n| (Box::leak(format!("{n:02}").into_boxed_str()) as &str, true))
                    .collect(),
            },
        ];
        let (pages, calls) = walk(&sources, 10);
        assert_eq!(pages.iter().map(Vec::len).sum::<usize>(), 39);
        // `a` is read once; `b` about twice per page (page batch + continuation probe).
        assert!(calls <= 1 + 2 * pages.len(), "{calls} fetches");
    }

    #[test]
    fn merge_cursor_round_trips_every_source_and_rejects_a_changed_source_set() {
        let sources = vec![
            SourceCursor {
                key: "git:local".into(),
                resume: None,
                exhausted: false,
            },
            SourceCursor {
                key: "provider:remote".into(),
                resume: Some("https://api.example/issues?page=3".into()),
                exhausted: true,
            },
        ];
        let last = MergeKey {
            native_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            qualified_id: "local:01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            title: "Boundary".into(),
            updated_at: "2026-09-01T00:00:00Z".into(),
            ..Default::default()
        };
        let keys = ["git:local".to_string(), "provider:remote".to_string()];
        let encoded =
            encode_cursor(&sources, SortKey::Priority, true, "filters-a", Some(&last)).unwrap();
        assert!(encoded.starts_with("v2."));
        let (decoded_last, decoded) =
            decode_cursor(Some(&encoded), &keys, SortKey::Priority, true, "filters-a").unwrap();
        assert_eq!(decoded_last, Some(last), "the value keyset round-trips");
        assert_eq!(decoded, sources);
        let stale = |result: Result<_, CheckoutPageError>| {
            assert_eq!(result.unwrap_err(), CheckoutPageError::StaleCursor);
        };
        stale(decode_cursor(
            Some(&encoded),
            &["git:local".into(), "provider:renamed".into()],
            SortKey::Priority,
            true,
            "filters-a",
        ));
        stale(decode_cursor(
            Some(&encoded),
            &keys,
            SortKey::Priority,
            true,
            "filters-b",
        ));
        stale(decode_cursor(
            Some(&encoded),
            &keys,
            SortKey::Title,
            true,
            "filters-a",
        ));
        stale(decode_cursor(
            Some(&encoded),
            &keys,
            SortKey::Priority,
            false,
            "filters-a",
        ));
        // Row-identity cursors from before value keysets cannot be reinterpreted.
        stale(decode_cursor(
            Some("v1.7b7d"),
            &keys,
            SortKey::Priority,
            true,
            "filters-a",
        ));
        for garbage in ["v2.zz", "v2.abc", "v2.7b7d", "v2.é0", "nonsense"] {
            assert_eq!(
                decode_cursor(Some(garbage), &keys, SortKey::Priority, true, "filters-a")
                    .unwrap_err(),
                CheckoutPageError::InvalidCursor,
                "{garbage}"
            );
        }
    }

    #[test]
    fn counts_parameter_is_an_explicit_boolean_and_omitted_counts_serialize_as_null() {
        assert_eq!(wants_counts(None), Ok(true));
        assert_eq!(wants_counts(Some("true")), Ok(true));
        assert_eq!(wants_counts(Some("false")), Ok(false));
        assert_eq!(
            wants_counts(Some("0")),
            Err(CheckoutPageError::InvalidCounts)
        );
        let page = serde_json::to_value(CheckoutTicketPage::default()).unwrap();
        assert_eq!(page, json!({ "items": [], "counts": null }));
    }

    #[test]
    fn counts_sum_summaries_and_widen_the_trend() {
        let days = ["d0", "d1", "d2"].map(String::from);
        assert_eq!(
            CheckoutTicketCounts::for_days(&days).completion_trend,
            [0, 0]
        );
        let mut counts = CheckoutTicketCounts::for_days(&days);
        counts.add(ProviderTicketSummary {
            total: 2,
            open: 1,
            completion_trend: vec![1, 0],
            ..Default::default()
        });
        counts.add(ProviderTicketSummary {
            total: 3,
            trash: 1,
            completion_trend: vec![0, 2, 5],
            ..Default::default()
        });
        assert_eq!(counts.total, 5);
        assert_eq!(counts.open, 1);
        assert_eq!(counts.trash, 1);
        assert_eq!(counts.completion_trend, [1, 2, 5]);
    }

    #[test]
    fn day_starts_default_to_seven_utc_days_and_validate_input() {
        let now = OffsetDateTime::parse("2026-09-25T13:00:00Z", &Rfc3339).unwrap();
        let starts = completion_day_starts(None, now).unwrap();
        assert_eq!(starts.len(), 8);
        assert_eq!(starts[6], "2026-09-25T00:00:00Z");
        assert_eq!(starts[7], "2026-09-26T00:00:00Z");
        let joined = starts.join(",");
        assert_eq!(completion_day_starts(Some(&joined), now).unwrap(), starts);
        for bad in [
            "",
            "2026-09-25T00:00:00Z",
            &starts.iter().rev().cloned().collect::<Vec<_>>().join(","),
        ] {
            assert_eq!(
                completion_day_starts(Some(bad), now).unwrap_err(),
                CheckoutPageError::InvalidSummaryDays
            );
        }
    }
}
