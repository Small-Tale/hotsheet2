//! Persisted per-store live state transitions (HS2-9AN4DN, docs/34-glance-surfaces.md §34.8).
//!
//! The server appends one compact JSON line each time a store's overnight state changes, to the
//! machine-local, gitignored `metrics/local/live-history.jsonl`. Records are append-only. Once the
//! file holds more than [`COMPACT_AFTER_LINES`] records, the next append rewrites it without the
//! records older than [`RETENTION_MS`], keeping the newest record before that cutoff so the state
//! at the start of any retained window stays known. Readers replay records into segments and skip
//! lines they cannot parse (a torn final line after a crash) or whose version they do not know.
//!
//! Record format, version 1, one JSON object per line:
//!
//! ```json
//! {"v":1,"at_ms":1762400000000,"state":"active"}
//! ```
//!
//! `state` is a [`HistoryState`]: the live state word, plus `waiting_on_ci` for a store whose
//! every live claim is in its final remote-CI phase.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The current record version. Readers skip other versions.
pub const RECORD_VERSION: u32 = 1;
/// Compaction drops records older than this (seven days).
pub const RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// Compact once the file holds more records than this.
pub const COMPACT_AFTER_LINES: usize = 4096;

/// A store's state on the overnight timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryState {
    Active,
    WaitingOnCi,
    MayBeStuck,
    WaitingOnYou,
    Stopped,
    Stalled,
    Halted,
}

/// One persisted transition: from `at_ms` the store was in `state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub v: u32,
    pub at_ms: i64,
    pub state: HistoryState,
}

/// One contiguous stretch in a single state, clipped to the requested window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistorySegment {
    pub state: HistoryState,
    pub start_ms: i64,
    pub end_ms: i64,
}

/// The history file of the store rooted at `root`.
#[must_use]
pub fn history_path(root: &Path) -> PathBuf {
    root.join("metrics")
        .join("local")
        .join("live-history.jsonl")
}

/// Parse every valid version-1 record in file order. Unreadable lines are skipped.
#[must_use]
pub fn parse_records(text: &str) -> Vec<HistoryRecord> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<HistoryRecord>(line).ok())
        .filter(|record| record.v == RECORD_VERSION)
        .collect()
}

/// Read `path`'s records; a missing file has none.
///
/// # Errors
/// Any read failure other than a missing file.
pub fn read_records(path: &Path) -> std::io::Result<Vec<HistoryRecord>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(parse_records(&text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

/// The records compaction keeps at `now_ms`: everything inside retention plus the newest record
/// before the cutoff, which states the store's state when the retained window opens.
#[must_use]
pub fn retained(records: &[HistoryRecord], now_ms: i64) -> Vec<HistoryRecord> {
    let cutoff = now_ms - RETENTION_MS;
    let start = match records.iter().position(|record| record.at_ms >= cutoff) {
        Some(0) => 0,
        Some(index) => index - 1,
        None => records.len().saturating_sub(1),
    };
    records[start..].to_vec()
}

/// Append `state` at `at_ms` unless it equals the last recorded state. Returns whether a record
/// was written. A time before the last record (a clock step backwards) is clamped to it.
///
/// # Errors
/// Reading, creating, or writing the file.
pub fn append_transition(path: &Path, at_ms: i64, state: HistoryState) -> std::io::Result<bool> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    let records = parse_records(&String::from_utf8_lossy(&bytes));
    if records.last().is_some_and(|last| last.state == state) {
        return Ok(false);
    }
    let record = HistoryRecord {
        v: RECORD_VERSION,
        at_ms: records.last().map_or(at_ms, |last| at_ms.max(last.at_ms)),
        state,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if records.len() >= COMPACT_AFTER_LINES {
        let mut kept = retained(&records, record.at_ms);
        kept.push(record);
        rewrite(path, &kept)?;
        return Ok(true);
    }
    let mut line = serde_json::to_string(&record).map_err(std::io::Error::other)?;
    line.push('\n');
    // A torn previous line must not swallow this record: start on a fresh line.
    if bytes.last().is_some_and(|byte| *byte != b'\n') {
        line.insert(0, '\n');
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(line.as_bytes())?;
    Ok(true)
}

fn rewrite(path: &Path, records: &[HistoryRecord]) -> std::io::Result<()> {
    let mut text = String::new();
    for record in records {
        text.push_str(&serde_json::to_string(record).map_err(std::io::Error::other)?);
        text.push('\n');
    }
    let temp = path.with_extension("jsonl.tmp");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)
}

/// One completed ticket on the timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryCompletion {
    pub slug: String,
    pub at_ms: i64,
}

/// The tickets' completion events (their own history, as the calibration report reads it) inside
/// `[from_ms, to_ms)`, oldest first.
#[must_use]
pub fn completions_between(
    tickets: &[hotsheet_model::Ticket],
    from_ms: i64,
    to_ms: i64,
) -> Vec<HistoryCompletion> {
    let mut out: Vec<HistoryCompletion> = tickets
        .iter()
        .flat_map(crate::calibration::ticket_completions)
        .filter_map(|completion| {
            let at = hotsheet_model::Timestamp::new(&completion.completed_at).instant()?;
            let at_ms = i64::try_from(at.unix_timestamp_nanos() / 1_000_000).ok()?;
            (at_ms >= from_ms && at_ms < to_ms).then_some(HistoryCompletion {
                slug: completion.slug,
                at_ms,
            })
        })
        .collect();
    out.sort_by(|a, b| a.at_ms.cmp(&b.at_ms).then(a.slug.cmp(&b.slug)));
    out
}

/// Replay `records` into the segments that overlap `[from_ms, to_ms)`, clipped to it. The state
/// before the first record is unknown and left out; each segment ends at the next record or at
/// `to_ms`. Neighbours in the same state merge.
#[must_use]
pub fn segments(records: &[HistoryRecord], from_ms: i64, to_ms: i64) -> Vec<HistorySegment> {
    let mut out: Vec<HistorySegment> = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let end = records
            .get(index + 1)
            .map_or(to_ms, |next| next.at_ms)
            .min(to_ms);
        let start = record.at_ms.max(from_ms);
        if end <= start {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.state == record.state && last.end_ms == start => last.end_ms = end,
            _ => out.push(HistorySegment {
                state: record.state,
                start_ms: start,
                end_ms: end,
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use HistoryState::{Active, Stalled, Stopped, WaitingOnCi, WaitingOnYou};

    const HOUR: i64 = 60 * 60 * 1000;

    fn record(at_ms: i64, state: HistoryState) -> HistoryRecord {
        HistoryRecord {
            v: RECORD_VERSION,
            at_ms,
            state,
        }
    }

    fn segment(state: HistoryState, start_ms: i64, end_ms: i64) -> HistorySegment {
        HistorySegment {
            state,
            start_ms,
            end_ms,
        }
    }

    #[test]
    fn hs2_9an4dn_record_format_is_one_versioned_line_per_transition() {
        let line = serde_json::to_string(&record(5, WaitingOnCi)).unwrap();
        assert_eq!(line, r#"{"v":1,"at_ms":5,"state":"waiting_on_ci"}"#);
        // Torn lines, unknown versions, and unknown states are skipped, not fatal.
        let text = format!(
            "{line}\n{{\"v\":2,\"at_ms\":6,\"state\":\"active\"}}\n{{\"v\":1,\"at_ms\":7,\"state\":\"dreaming\"}}\nnot json\n{{\"v\":1,\"at_ms\":8,\"st"
        );
        assert_eq!(parse_records(&text), vec![record(5, WaitingOnCi)]);
    }

    #[test]
    fn hs2_9an4dn_append_writes_only_changes_and_clamps_a_clock_step_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = history_path(dir.path());
        assert!(
            read_records(&path).unwrap().is_empty(),
            "a missing file has no records"
        );
        assert!(append_transition(&path, 10, Active).unwrap());
        assert!(
            !append_transition(&path, 20, Active).unwrap(),
            "same state again"
        );
        assert!(append_transition(&path, 30, Stalled).unwrap());
        assert!(
            append_transition(&path, 25, Active).unwrap(),
            "clock stepped back"
        );
        assert_eq!(
            read_records(&path).unwrap(),
            vec![record(10, Active), record(30, Stalled), record(30, Active)]
        );
    }

    #[test]
    fn hs2_9an4dn_append_after_a_torn_line_starts_a_fresh_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = history_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "{\"v\":1,\"at_ms\":1,\"state\":\"active\"}\n{\"v\":1,\"at_",
        )
        .unwrap();
        assert!(append_transition(&path, 5, Stopped).unwrap());
        assert_eq!(
            read_records(&path).unwrap(),
            vec![record(1, Active), record(5, Stopped)]
        );
    }

    #[test]
    fn hs2_9an4dn_compaction_keeps_retention_and_the_opening_state() {
        let now = 30 * 24 * HOUR;
        let old = vec![
            record(now - 10 * 24 * HOUR, Active),
            record(now - 9 * 24 * HOUR, Stopped),
            record(now - 2 * HOUR, Active),
            record(now - HOUR, Stalled),
        ];
        // The newest record before the cutoff opens the retained window.
        assert_eq!(retained(&old, now), old[1..].to_vec());
        // Nothing inside retention: only the newest record survives.
        assert_eq!(retained(&old[..2], now), vec![old[1]]);
        assert!(retained(&[], now).is_empty());
        assert_eq!(retained(&old[2..], now), old[2..].to_vec());

        let dir = tempfile::tempdir().unwrap();
        let path = history_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut text = String::new();
        for index in 0..COMPACT_AFTER_LINES {
            let state = if index % 2 == 0 { Active } else { Stopped };
            let at = now - 20 * 24 * HOUR + i64::try_from(index).unwrap();
            text.push_str(&serde_json::to_string(&record(at, state)).unwrap());
            text.push('\n');
        }
        std::fs::write(&path, text).unwrap();
        assert!(append_transition(&path, now, WaitingOnYou).unwrap());
        let kept = read_records(&path).unwrap();
        assert_eq!(kept.len(), 2, "the opening record and the new one");
        assert_eq!(kept[1], record(now, WaitingOnYou));
        assert!(!path.with_extension("jsonl.tmp").exists());
    }

    #[test]
    fn hs2_9an4dn_replay_clips_merges_and_leaves_unknown_time_out() {
        assert!(segments(&[], 0, 100).is_empty(), "empty history");
        // A single state that began before the window fills it.
        assert_eq!(
            segments(&[record(-50, Active)], 0, 100),
            vec![segment(Active, 0, 100)]
        );
        // Before the first record the state is unknown.
        assert_eq!(
            segments(&[record(40, Active)], 0, 100),
            vec![segment(Active, 40, 100)]
        );
        // Interleaved states; equal neighbours (as after a compaction) merge.
        let records = [
            record(0, Active),
            record(20, Stalled),
            record(30, Active),
            record(30, Active),
            record(60, Stopped),
            record(150, Active),
        ];
        assert_eq!(
            segments(&records, 10, 100),
            vec![
                segment(Active, 10, 20),
                segment(Stalled, 20, 30),
                segment(Active, 30, 60),
                segment(Stopped, 60, 100),
            ]
        );
        // A window entirely after the last record, and an inverted window.
        assert_eq!(
            segments(&records, 200, 300),
            vec![segment(Active, 200, 300)]
        );
        assert!(segments(&records, 50, 50).is_empty());
        assert!(segments(&records, 90, 10).is_empty());
    }
}
