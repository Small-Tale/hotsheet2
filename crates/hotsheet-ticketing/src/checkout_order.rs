//! The single total order used to combine tickets from several checkout sources.
//!
//! A checkout can span multiple git stores and hosted-provider connections. Each source
//! sorts its own rows, and callers combine them with this comparator so every listing
//! shape — bounded pages, capped arrays, and the serverless MCP backend — yields one
//! global order instead of source-by-source concatenation (HS2-2BDSRK, HS2-M0YTB6).

use crate::SortKey;
use crate::wire::{ApiTicket, TicketRow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;

/// Most rows one checkout read may return, paged or unpaged (HS2-CYXS0N). Larger reads
/// page with `page_size` + `cursor`; an unpaged read that would exceed it fails explicitly.
pub const CHECKOUT_READ_MAX_ROWS: usize = 500;

/// The sortable projection of one ticket row from any checkout source.
///
/// It is also the value-keyset continuation position (HS2-74H84S): a checkout cursor records
/// the last emitted row's key, and every source resumes strictly after it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeKey {
    pub native_id: String,
    pub qualified_id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub priority_rank: u8,
    pub status_rank: u8,
}

impl MergeKey {
    /// Read the key from a serialized ticket row (a `TicketRow` or provider ticket).
    /// Missing fields sort as empty / unknown rather than failing the listing.
    #[must_use]
    pub fn from_json(value: &Value) -> Self {
        let text = |key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default();
        Self {
            native_id: text("native_id").to_owned(),
            qualified_id: text("qualified_id").to_owned(),
            title: text("title").to_owned(),
            created_at: text("created_at").to_owned(),
            updated_at: text("updated_at").to_owned(),
            priority_rank: priority_rank(Some(text("priority"))),
            status_rank: status_rank(Some(text("status"))),
        }
    }

    /// Key of an indexed local row, after `TicketRow::set_connection`.
    #[must_use]
    pub fn from_row(row: &TicketRow) -> Self {
        Self {
            native_id: row.native_id.clone(),
            qualified_id: row.qualified_id.clone(),
            title: row.title.clone(),
            created_at: row.created_at.clone().unwrap_or_default(),
            updated_at: row.updated_at.clone().unwrap_or_default(),
            priority_rank: priority_rank(row.priority.as_deref()),
            status_rank: status_rank(row.status.as_deref()),
        }
    }

    /// Key of a provider ticket.
    #[must_use]
    pub fn from_ticket(ticket: &ApiTicket) -> Self {
        Self {
            native_id: ticket.native_id.clone(),
            qualified_id: ticket.qualified_id.clone(),
            title: ticket.title.clone(),
            created_at: ticket.created_at.clone(),
            updated_at: ticket.updated_at.clone(),
            priority_rank: ticket.priority as u8,
            status_rank: ticket.status as u8,
        }
    }
}

/// A value-keyset predicate for one source read (HS2-74H84S): return only rows sorting
/// strictly after `key` in [`compare`] order. `connection_id` is the queried source's
/// connection, which forms its rows' qualified ids (`{connection_id}:{native_id}`) for the
/// final cross-source tiebreaker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AfterKey {
    pub key: MergeKey,
    pub connection_id: String,
}

/// Fold a title for case-insensitive ordering. ASCII-only, matching SQLite's built-in
/// `lower()` used by the index, so local SQL keysets and in-memory merges agree exactly.
#[must_use]
pub fn title_fold(title: &str) -> String {
    title.to_ascii_lowercase()
}

/// Rank a wire priority name in `Priority` declaration order; unknown sorts last.
#[must_use]
pub fn priority_rank(value: Option<&str>) -> u8 {
    match value {
        Some("highest") => 0,
        Some("high") => 1,
        Some("default") => 2,
        Some("low") => 3,
        Some("lowest") => 4,
        _ => u8::MAX,
    }
}

/// Rank a wire status name in `Status` declaration order; unknown sorts last.
#[must_use]
pub fn status_rank(value: Option<&str>) -> u8 {
    match value {
        Some("not_started") => 0,
        Some("started") => 1,
        Some("completed") => 2,
        Some("verified") => 3,
        Some("backlog") => 4,
        Some("archive") => 5,
        Some("deleted") => 6,
        Some("moved") => 7,
        _ => u8::MAX,
    }
}

/// Compare two rows in the requested checkout order. Chronological and id sorts fall
/// back to native then qualified identity in the same direction; categorical sorts
/// break ties recent-first, then by qualified identity, regardless of direction.
#[must_use]
pub fn compare(left: &MergeKey, right: &MergeKey, sort: SortKey, descending: bool) -> Ordering {
    let directed = |order: Ordering| {
        if descending { order.reverse() } else { order }
    };
    match sort {
        SortKey::Id => directed(left.native_id.cmp(&right.native_id))
            .then_with(|| directed(left.qualified_id.cmp(&right.qualified_id))),
        SortKey::Created => directed(left.created_at.cmp(&right.created_at))
            .then_with(|| directed(left.native_id.cmp(&right.native_id)))
            .then_with(|| directed(left.qualified_id.cmp(&right.qualified_id))),
        SortKey::Updated => directed(left.updated_at.cmp(&right.updated_at))
            .then_with(|| directed(left.native_id.cmp(&right.native_id)))
            .then_with(|| directed(left.qualified_id.cmp(&right.qualified_id))),
        SortKey::Priority => directed(left.priority_rank.cmp(&right.priority_rank))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.qualified_id.cmp(&right.qualified_id)),
        SortKey::Status => directed(left.status_rank.cmp(&right.status_rank))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.qualified_id.cmp(&right.qualified_id)),
        SortKey::Title => directed(title_fold(&left.title).cmp(&title_fold(&right.title)))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.qualified_id.cmp(&right.qualified_id)),
    }
}

/// Globally order serialized rows gathered from several sources, then keep at most
/// `limit`. Callers must read each source without its own `limit` truncation (or with
/// the same `limit`, which is still sufficient because the global top-N draws at most N
/// rows from any one source).
#[must_use]
pub fn merge_rows(
    rows: Vec<Value>,
    sort: SortKey,
    descending: bool,
    limit: Option<usize>,
) -> Vec<Value> {
    let mut keyed = rows
        .into_iter()
        .map(|row| (MergeKey::from_json(&row), row))
        .collect::<Vec<_>>();
    keyed.sort_by(|(left, _), (right, _)| compare(left, right, sort, descending));
    keyed
        .into_iter()
        .take(limit.unwrap_or(usize::MAX))
        .map(|(_, row)| row)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(id: &str, title: &str, updated_at: &str, priority_rank: u8) -> MergeKey {
        MergeKey {
            native_id: id.into(),
            qualified_id: id.into(),
            title: title.into(),
            created_at: "2026-09-01T00:00:00Z".into(),
            updated_at: updated_at.into(),
            priority_rank,
            status_rank: 0,
        }
    }

    #[test]
    fn categorical_order_keeps_recent_first_stable_ties() {
        let older = key("git:2", "Same", "2026-09-01T00:00:00Z", 1);
        let newer = key("provider:1", "same", "2026-09-02T00:00:00Z", 1);
        assert_eq!(
            compare(&newer, &older, SortKey::Title, false),
            Ordering::Less
        );
        assert_eq!(
            compare(&newer, &older, SortKey::Priority, true),
            Ordering::Less,
            "descending changes only the categorical primary key"
        );
        let same_time = key("git:1", "Same", "2026-09-02T00:00:00Z", 1);
        assert_eq!(
            compare(&same_time, &newer, SortKey::Title, false),
            Ordering::Less,
            "qualified identity is the cross-source final tiebreaker"
        );
    }

    #[test]
    fn ranks_follow_model_declaration_order_and_unknown_sorts_last() {
        assert!(priority_rank(Some("highest")) < priority_rank(Some("lowest")));
        assert_eq!(priority_rank(Some("bogus")), u8::MAX);
        assert_eq!(priority_rank(None), u8::MAX);
        assert!(status_rank(Some("not_started")) < status_rank(Some("moved")));
        assert_eq!(status_rank(Some("")), u8::MAX);
    }

    #[test]
    fn merge_rows_interleaves_sources_and_caps_globally() {
        let row = |source: &str, id: &str, title: &str, priority: &str| {
            json!({
                "native_id": id,
                "qualified_id": format!("{source}:{id}"),
                "title": title,
                "priority": priority,
                "status": "not_started",
                "updated_at": "2026-09-01T00:00:00Z",
            })
        };
        // Two sources, each already sorted by title, concatenated source-by-source.
        let rows = vec![
            row("git", "1", "Bravo", "low"),
            row("git", "2", "Delta", "high"),
            row("gh", "7", "Alpha", "default"),
            row("gh", "8", "Charlie", "highest"),
        ];
        let titles = |rows: &[Value]| {
            rows.iter()
                .map(|row| row["title"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        let by_title = merge_rows(rows.clone(), SortKey::Title, false, None);
        assert_eq!(titles(&by_title), ["Alpha", "Bravo", "Charlie", "Delta"]);
        let capped = merge_rows(rows.clone(), SortKey::Title, false, Some(3));
        assert_eq!(titles(&capped), ["Alpha", "Bravo", "Charlie"]);
        let by_priority = merge_rows(rows.clone(), SortKey::Priority, false, Some(2));
        assert_eq!(titles(&by_priority), ["Charlie", "Delta"]);
        let reversed = merge_rows(rows, SortKey::Title, true, Some(0));
        assert!(reversed.is_empty(), "limit=0 returns no rows");
    }

    #[test]
    fn title_order_folds_ascii_case_only_matching_the_sqlite_index() {
        let upper = key("1", "BETA", "2026-09-01T00:00:00Z", 0);
        let lower = key("2", "alpha", "2026-09-01T00:00:00Z", 0);
        let accented = key("3", "Élan", "2026-09-01T00:00:00Z", 0);
        assert_eq!(
            compare(&lower, &upper, SortKey::Title, false),
            Ordering::Less
        );
        // SQLite's built-in lower() leaves non-ASCII bytes alone, so neither side folds É.
        assert_eq!(title_fold("Élan"), "Élan");
        assert_eq!(
            compare(&upper, &accented, SortKey::Title, false),
            Ordering::Less
        );
    }

    #[test]
    fn key_round_trips_through_serde_for_cursors() {
        let original = key(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "Title",
            "2026-09-01T00:00:00Z",
            3,
        );
        let encoded = serde_json::to_string(&original).unwrap();
        assert_eq!(
            serde_json::from_str::<MergeKey>(&encoded).unwrap(),
            original
        );
    }

    #[test]
    fn from_json_tolerates_projected_rows() {
        let key = MergeKey::from_json(&json!({ "slug": "HS-1" }));
        assert_eq!(key.priority_rank, u8::MAX);
        assert_eq!(key.status_rank, u8::MAX);
        assert!(key.qualified_id.is_empty());
    }
}
