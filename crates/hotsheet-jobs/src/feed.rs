//! The bounded append-only feed (`feed.jsonl`). Sequences only grow; compaction drops the
//! oldest entries and moves `first_seq`, so a reader behind it gets an explicit gap.
//!
//! Feed text can be local process output. It stays in this file: it is never copied into
//! the snapshot, an outcome, a ticket, a notification or telemetry.

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::model::FeedState;

/// A longer line is cut at a character boundary.
pub const MAX_FEED_LINE_BYTES: usize = 4096;
/// The most lines one update may append.
pub const MAX_FEED_BATCH: usize = 256;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedStream {
    Out,
    Err,
    Info,
}

/// One line a producer appends.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedLine {
    pub stream: FeedStream,
    pub text: String,
}

impl FeedLine {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            stream: FeedStream::Info,
            text: text.into(),
        }
    }

    pub fn out(text: impl Into<String>) -> Self {
        Self {
            stream: FeedStream::Out,
            text: text.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FeedEntry {
    pub seq: u64,
    pub at_ms: u64,
    pub stream: FeedStream,
    pub text: String,
}

/// Sequences `from_seq..=to_seq` were removed by retention before the reader saw them.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FeedGap {
    pub from_seq: u64,
    pub to_seq: u64,
}

/// Entries after a cursor. Pass `cursor` back to continue; start from 0.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FeedPage {
    pub entries: Vec<FeedEntry>,
    pub gap: Option<FeedGap>,
    pub cursor: u64,
}

pub(crate) enum FeedWrite {
    Append(Vec<u8>),
    Rewrite(Vec<u8>),
}

/// The feed change for one batch, computed before the snapshot is written. The snapshot
/// (carrying the advanced `state`) is persisted first and the feed second, so a crash
/// between the two skips sequence numbers but never reuses one.
pub(crate) fn plan(
    path: &Path,
    state: &mut FeedState,
    lines: &[FeedLine],
    now_ms: u64,
    cap_bytes: u64,
) -> io::Result<FeedWrite> {
    let mut fresh = Vec::with_capacity(lines.len());
    for line in lines {
        fresh.push(FeedEntry {
            seq: state.next_seq,
            at_ms: now_ms,
            stream: line.stream,
            text: truncate(&line.text).to_owned(),
        });
        state.next_seq += 1;
    }
    let fresh_bytes = encode(&fresh)?;
    let (existing_len, torn) = tail_state(path)?;
    if !torn && existing_len + fresh_bytes.len() as u64 <= cap_bytes {
        return Ok(FeedWrite::Append(fresh_bytes));
    }
    // Over the cap, or a crash left a partial last line: keep the newest half-cap.
    let mut entries = read_entries(path, state.first_seq)?;
    entries.extend(fresh);
    let mut sizes = Vec::with_capacity(entries.len());
    let mut total = 0_u64;
    for entry in &entries {
        let size = encode(std::slice::from_ref(entry))?.len() as u64;
        sizes.push(size);
        total += size;
    }
    let target = if torn && total <= cap_bytes {
        cap_bytes
    } else {
        cap_bytes / 2
    };
    let mut drop = 0;
    while drop < entries.len() && total > target {
        total -= sizes[drop];
        drop += 1;
    }
    let kept = &entries[drop..];
    state.first_seq = kept.first().map_or(state.next_seq, |entry| entry.seq);
    Ok(FeedWrite::Rewrite(encode(kept)?))
}

pub(crate) fn apply(path: &Path, write: FeedWrite) -> io::Result<()> {
    match write {
        FeedWrite::Append(bytes) => {
            if bytes.is_empty() {
                return Ok(());
            }
            let mut file = OpenOptions::new().create(true).append(true).open(path)?;
            file.write_all(&bytes)
        }
        FeedWrite::Rewrite(bytes) => {
            let dir = path.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "feed path has no parent")
            })?;
            let mut temp = tempfile::NamedTempFile::new_in(dir)?;
            temp.write_all(&bytes)?;
            temp.flush()?;
            temp.persist(path).map_err(|error| error.error)?;
            Ok(())
        }
    }
}

pub(crate) fn read_page(
    path: &Path,
    state: FeedState,
    after: u64,
    limit: usize,
) -> io::Result<FeedPage> {
    let newest = state.next_seq.saturating_sub(1);
    let after = after.min(newest);
    let gap = (after + 1 < state.first_seq).then(|| FeedGap {
        from_seq: after + 1,
        to_seq: state.first_seq - 1,
    });
    let floor = after.max(state.first_seq.saturating_sub(1));
    let entries: Vec<FeedEntry> = read_entries(path, state.first_seq)?
        .into_iter()
        .filter(|entry| entry.seq > floor && entry.seq <= newest)
        .take(limit)
        .collect();
    let cursor = entries.last().map_or(floor, |entry| entry.seq);
    Ok(FeedPage {
        entries,
        gap,
        cursor,
    })
}

/// Parsed entries at or after `first_seq`, in strictly increasing order. A partial or
/// damaged line is skipped rather than failing the whole feed.
fn read_entries(path: &Path, first_seq: u64) -> io::Result<Vec<FeedEntry>> {
    let mut text = String::new();
    match fs::File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(u64::MAX).read_to_end(&mut bytes)?;
            text = String::from_utf8_lossy(&bytes).into_owned();
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut entries: Vec<FeedEntry> = Vec::new();
    for line in text.lines() {
        let Ok(entry) = serde_json::from_str::<FeedEntry>(line) else {
            continue;
        };
        let ordered = entries.last().is_none_or(|last| entry.seq > last.seq);
        if entry.seq >= first_seq && ordered {
            entries.push(entry);
        }
    }
    Ok(entries)
}

/// The file length and whether its last line lacks a newline (a torn write).
fn tail_state(path: &Path) -> io::Result<(u64, bool)> {
    use std::io::{Seek, SeekFrom};

    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((0, false)),
        Err(error) => return Err(error),
    };
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok((0, false));
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0_u8; 1];
    file.read_exact(&mut last)?;
    Ok((len, last[0] != b'\n'))
}

fn encode(entries: &[FeedEntry]) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for entry in entries {
        serde_json::to_writer(&mut bytes, entry)?;
        bytes.push(b'\n');
    }
    Ok(bytes)
}

fn truncate(text: &str) -> &str {
    if text.len() <= MAX_FEED_LINE_BYTES {
        return text;
    }
    let mut end = MAX_FEED_LINE_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push(path: &Path, state: &mut FeedState, texts: &[&str], cap: u64) {
        let lines: Vec<FeedLine> = texts.iter().map(|text| FeedLine::out(*text)).collect();
        let write = plan(path, state, &lines, 10, cap).unwrap();
        apply(path, write).unwrap();
    }

    fn seqs(page: &FeedPage) -> Vec<u64> {
        page.entries.iter().map(|entry| entry.seq).collect()
    }

    #[test]
    fn cursor_replay_returns_only_newer_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feed.jsonl");
        let mut state = FeedState::default();
        let empty = read_page(&path, state, 0, 10).unwrap();
        assert_eq!((empty.entries.len(), empty.gap, empty.cursor), (0, None, 0));

        push(&path, &mut state, &["a", "b", "c"], 1 << 20);
        let first = read_page(&path, state, 0, 2).unwrap();
        assert_eq!(seqs(&first), [1, 2]);
        assert_eq!(first.gap, None);
        let rest = read_page(&path, state, first.cursor, 10).unwrap();
        assert_eq!(seqs(&rest), [3]);
        let idle = read_page(&path, state, rest.cursor, 10).unwrap();
        assert_eq!((seqs(&idle), idle.cursor), (vec![], 3));

        push(&path, &mut state, &["d"], 1 << 20);
        assert_eq!(seqs(&read_page(&path, state, 3, 10).unwrap()), [4]);
    }

    #[test]
    fn compaction_reports_a_gap_then_resumes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feed.jsonl");
        let mut state = FeedState::default();
        let line = "x".repeat(100);
        for _ in 0..40 {
            push(&path, &mut state, &[&line], 2048);
            assert!(fs::metadata(&path).unwrap().len() <= 2048);
        }
        assert_eq!(state.next_seq, 41);
        assert!(state.first_seq > 1);
        let page = read_page(&path, state, 0, 100).unwrap();
        assert_eq!(
            page.gap,
            Some(FeedGap {
                from_seq: 1,
                to_seq: state.first_seq - 1
            })
        );
        assert_eq!(page.entries.first().unwrap().seq, state.first_seq);
        assert_eq!(page.cursor, 40);
        let sequence = seqs(&page);
        assert!(sequence.windows(2).all(|pair| pair[1] == pair[0] + 1));
        // A reader inside the retained range sees no gap.
        let inside = read_page(&path, state, state.first_seq, 100).unwrap();
        assert_eq!(inside.gap, None);
    }

    #[test]
    fn an_oversized_batch_keeps_only_its_newest_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feed.jsonl");
        let mut state = FeedState::default();
        let line = "y".repeat(200);
        let texts: Vec<&str> = (0..50).map(|_| line.as_str()).collect();
        push(&path, &mut state, &texts, 2048);
        assert_eq!(state.next_seq, 51);
        let page = read_page(&path, state, 0, 100).unwrap();
        assert_eq!(page.entries.last().unwrap().seq, 50);
        assert_eq!(page.gap.unwrap().from_seq, 1);
        assert!(fs::metadata(&path).unwrap().len() <= 1024);
    }

    #[test]
    fn a_torn_last_line_is_dropped_and_the_next_append_stays_parseable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feed.jsonl");
        let mut state = FeedState::default();
        push(&path, &mut state, &["a", "b"], 1 << 20);
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"{\"seq\":3,\"at_ms\":1,\"stre").unwrap();
        drop(file);
        // The snapshot had reserved seq 3 before the crash.
        state.next_seq = 4;
        assert_eq!(seqs(&read_page(&path, state, 0, 10).unwrap()), [1, 2]);
        push(&path, &mut state, &["d"], 1 << 20);
        let page = read_page(&path, state, 0, 10).unwrap();
        assert_eq!(seqs(&page), [1, 2, 4]);
        assert_eq!(page.gap, None);
        assert!(fs::read_to_string(&path).unwrap().ends_with('\n'));
    }

    #[test]
    fn a_cursor_beyond_the_feed_is_clamped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feed.jsonl");
        let mut state = FeedState::default();
        push(&path, &mut state, &["a"], 1 << 20);
        let page = read_page(&path, state, 99, 10).unwrap();
        assert_eq!((seqs(&page), page.gap, page.cursor), (vec![], None, 1));
    }

    #[test]
    fn long_lines_are_cut_at_a_character_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feed.jsonl");
        let mut state = FeedState::default();
        let long = "\u{e9}".repeat(MAX_FEED_LINE_BYTES);
        push(&path, &mut state, &[&long], 1 << 20);
        let page = read_page(&path, state, 0, 10).unwrap();
        let text = &page.entries[0].text;
        assert_eq!(text.len(), MAX_FEED_LINE_BYTES);
        assert!(text.chars().all(|c| c == '\u{e9}'));
    }

    #[test]
    fn emptied_then_refilled_feed_keeps_monotonic_sequences() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feed.jsonl");
        let mut state = FeedState::default();
        push(&path, &mut state, &["a", "b"], 1 << 20);
        fs::remove_file(&path).unwrap();
        assert!(read_page(&path, state, 0, 10).unwrap().entries.is_empty());
        push(&path, &mut state, &["c"], 1 << 20);
        let page = read_page(&path, state, 2, 10).unwrap();
        assert_eq!(seqs(&page), [3]);
    }
}
