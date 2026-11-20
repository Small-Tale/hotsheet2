//! Numeric usage from the new portion of one interactive Claude transcript.
//!
//! Claude's documented turn hooks provide the transcript path but no token counters. The
//! JSONL row shape is not a hook contract, so unknown or malformed rows are ignored and no
//! conversation content is retained. Repeated rows for one assistant message count once.

use crate::drive::Usage;
use hotsheet_ticketing::metrics::ClaudeTokenCategories;
use hotsheet_ticketing::pricing::Prices;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

const MAX_TURN_BYTES: u64 = 64 * 1024 * 1024;

/// The usual turn aggregate plus optional categories safe to retain for later repricing.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeTurnUsage {
    pub usage: Usage,
    pub categories: Option<ClaudeTokenCategories>,
}

/// Read only bytes appended after a `UserPromptSubmit` hook's file-size snapshot.
/// A truncated/replaced transcript or an unbounded turn produces no guessed usage.
pub fn usage_since(path: &Path, offset: u64) -> io::Result<Option<Usage>> {
    usage_since_with_prices(path, offset, None).map(|turn| turn.map(|turn| turn.usage))
}

/// Price the transcript's separate cache categories instead of treating every input
/// token as an uncached input token.
pub fn priced_usage_since(path: &Path, offset: u64, prices: &Prices) -> io::Result<Option<Usage>> {
    priced_usage_with_evidence_since(path, offset, prices).map(|turn| turn.map(|turn| turn.usage))
}

/// Return the numeric evidence with the same bounded, de-duplicated turn scan used for
/// initial pricing. A mixed-model or malformed category turn has no repricing evidence.
pub fn priced_usage_with_evidence_since(
    path: &Path,
    offset: u64,
    prices: &Prices,
) -> io::Result<Option<ClaudeTurnUsage>> {
    usage_since_with_prices(path, offset, Some(prices))
}

fn usage_since_with_prices(
    path: &Path,
    offset: u64,
    prices: Option<&Prices>,
) -> io::Result<Option<ClaudeTurnUsage>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    if len < offset {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude transcript became shorter than the turn start",
        ));
    }
    if len - offset > MAX_TURN_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude turn transcript exceeds the bounded usage scan",
        ));
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::with_capacity((len - offset) as usize);
    file.take(MAX_TURN_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_TURN_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude turn transcript exceeds the bounded usage scan",
        ));
    }
    Ok(usage_from_lines(&bytes, prices))
}

/// The numeric usage of one assistant transcript row of the wanted chain. Only the message
/// id, timestamp, model, stop reason, and counters are read; content never leaves this
/// function.
struct MessageRow {
    id: String,
    timestamp: String,
    usage: Usage,
    cost: Option<f64>,
    categories: Option<ClaudeTokenCategories>,
    /// The row names a stop reason: Claude wrote the message's final counters (HS2-JW6N67).
    complete: bool,
}

/// `sidechain` selects a subagent transcript's rows (`isSidechain: true`, HS2-JW6N67);
/// otherwise only main-chain rows count, so a subagent row in the main file is never
/// mistaken for the session's own response.
fn message_row(line: &[u8], prices: Option<&Prices>, sidechain: bool) -> Option<MessageRow> {
    let value = serde_json::from_slice::<Value>(line).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("assistant")
        || (value.get("isSidechain").and_then(Value::as_bool) == Some(true)) != sidechain
        || value
            .get("parent_tool_use_id")
            .is_some_and(|parent| !parent.is_null())
    {
        return None;
    }
    let message = value.get("message")?;
    let id = message.get("id").and_then(Value::as_str)?;
    let usage = crate::claude::claude_result_usage(message)?;
    let categories = message.get("usage").and_then(categories_from_usage);
    let cost = prices.and_then(|prices| {
        let model = usage.model.as_ref()?;
        let price = prices.get(model)?;
        // Keep initial pricing's established zero-default for sparse transcript rows.
        // Repricing evidence is stricter: it is omitted unless counters are explicit.
        let counters = message.get("usage")?;
        let count = |key| counters.get(key).and_then(Value::as_u64).unwrap_or(0);
        let cache_creation = counters.get("cache_creation");
        let cache_count = |key| {
            cache_creation
                .and_then(|value| value.get(key))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        };
        let write_5m = cache_count("ephemeral_5m_input_tokens");
        let write_1h = cache_count("ephemeral_1h_input_tokens");
        price.claude_cost(
            count("input_tokens"),
            count("cache_read_input_tokens"),
            write_5m,
            write_1h,
            count("cache_creation_input_tokens").saturating_sub(write_5m.saturating_add(write_1h)),
            count("output_tokens"),
        )
    });
    if usage.tokens_in == 0 && usage.tokens_out == 0 {
        return None;
    }
    let complete = message
        .get("stop_reason")
        .and_then(Value::as_str)
        .is_some_and(|reason| !reason.is_empty());
    Some(MessageRow {
        complete,
        id: id.to_owned(),
        timestamp: value
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        usage,
        cost,
        categories,
    })
}

fn usage_from_lines(bytes: &[u8], prices: Option<&Prices>) -> Option<ClaudeTurnUsage> {
    let mut messages =
        BTreeMap::<String, (String, Usage, Option<f64>, Option<ClaudeTokenCategories>)>::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        let Some(row) = message_row(line, prices, false) else {
            continue;
        };
        // A streamed assistant message may appear on several rows. The latest snapshot of
        // its counters is authoritative; stale rows received out of order cannot lower it.
        if messages
            .get(&row.id)
            .is_none_or(|(previous, _, _, _)| row.timestamp >= *previous)
        {
            messages.insert(row.id, (row.timestamp, row.usage, row.cost, row.categories));
        }
    }
    if messages.is_empty() {
        return None;
    }
    let mut total = Usage::default();
    let mut model = None::<String>;
    let mut mixed_models = false;
    let mut total_cost = prices.map(|_| 0.0);
    let mut categories = Some(ClaudeTokenCategories::default());
    for (_, usage, cost, message_categories) in messages.values() {
        total.tokens_in = total.tokens_in.saturating_add(usage.tokens_in);
        total.tokens_out = total.tokens_out.saturating_add(usage.tokens_out);
        total_cost = total_cost.zip(*cost).map(|(sum, amount)| sum + amount);
        categories = categories
            .zip(*message_categories)
            .map(|(mut total, next)| {
                total.add(next);
                total
            });
        match &usage.model {
            Some(current) if model.as_ref().is_none_or(|known| known == current) => {
                model = Some(current.clone());
            }
            _ => mixed_models = true,
        }
    }
    total.model = if mixed_models { None } else { model };
    total.cost_usd = total_cost;
    Some(ClaudeTurnUsage {
        categories: categories.filter(|_| total.model.is_some()),
        usage: total,
    })
}

/// Only retain a snapshot when the observed Claude counters are numeric and their cache
/// write subtotals fit the reported total. Missing TTL categories become unclassified.
fn categories_from_usage(usage: &Value) -> Option<ClaudeTokenCategories> {
    let count = |key| usage.get(key).and_then(Value::as_u64);
    let uncached_input = count("input_tokens")?;
    let cache_read = count("cache_read_input_tokens")?;
    let cache_write = count("cache_creation_input_tokens")?;
    let output = count("output_tokens")?;
    let ttl = usage.get("cache_creation");
    let ttl_count = |key| {
        ttl.and_then(|value| value.get(key))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    let cache_write_5m = ttl_count("ephemeral_5m_input_tokens");
    let cache_write_1h = ttl_count("ephemeral_1h_input_tokens");
    let classified = cache_write_5m.checked_add(cache_write_1h)?;
    Some(ClaudeTokenCategories {
        uncached_input,
        cache_read,
        cache_write_5m,
        cache_write_1h,
        cache_write_unclassified: cache_write.checked_sub(classified)?,
        output,
    })
}

/// One main-chain assistant message's final numeric usage (HS2-SSMDQ1).
#[derive(Debug, Clone, PartialEq)]
pub struct MessageUsage {
    /// Claude's message id: the identity that keeps a message counted once.
    pub message_id: String,
    /// The transcript row's timestamp, as written (may be empty).
    pub timestamp: String,
    pub usage: Usage,
    pub categories: Option<ClaudeTokenCategories>,
}

struct PendingMessage {
    /// Byte offset of the line that first named this message; a resume reads from here.
    first_offset: u64,
    message: MessageUsage,
    /// A row of this message named a stop reason, so its counters are final (HS2-JW6N67).
    complete: bool,
}

/// Bytes read per chunk while following; complete lines only are consumed.
const FOLLOW_CHUNK: u64 = 8 * 1024 * 1024;

/// Follows one interactive Claude transcript and yields each assistant message's usage once
/// it is final (HS2-SSMDQ1). A message is final when a row of a later message appears, when
/// one of its rows names a stop reason (HS2-JW6N67), or when the caller says the turn
/// ended. Repeated rows of one message keep its latest counter snapshot, as the turn scan
/// does. Only usage fields are retained.
pub struct TranscriptFollower {
    offset: u64,
    pending: Vec<PendingMessage>,
    sidechain: bool,
}

impl TranscriptFollower {
    /// Start following a session's main transcript after byte `offset` (a prompt-time size,
    /// or a persisted resume point).
    pub fn new(offset: u64) -> Self {
        Self {
            offset,
            pending: Vec::new(),
            sidechain: false,
        }
    }

    /// Follow one background subagent's own transcript (`<session>/subagents/agent-<id>.jsonl`),
    /// whose rows are all sidechain rows (HS2-JW6N67).
    pub fn subagent(offset: u64) -> Self {
        Self {
            sidechain: true,
            ..Self::new(offset)
        }
    }

    /// The point a later follower must resume from so no unrecorded message is skipped:
    /// the first line of the oldest pending message, else everything read so far.
    pub fn resume_offset(&self) -> u64 {
        self.pending
            .first()
            .map_or(self.offset, |pending| pending.first_offset)
    }

    /// Whether any message has been read but not yet handed out as recorded.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Read complete lines appended since the last call. A trailing line without its newline
    /// waits for the next call; a transcript that shrank (replaced) is read again from the
    /// start, which callers keep idempotent by message id. A missing file reads nothing.
    pub fn read_appended(&mut self, path: &Path, prices: Option<&Prices>) -> io::Result<()> {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let len = file.metadata()?.len();
        if len < self.offset {
            self.offset = 0;
            self.pending.clear();
        }
        while self.offset < len {
            file.seek(SeekFrom::Start(self.offset))?;
            let mut bytes = Vec::new();
            (&mut file)
                .take((len - self.offset).min(FOLLOW_CHUNK))
                .read_to_end(&mut bytes)?;
            let Some(end) = bytes.iter().rposition(|byte| *byte == b'\n') else {
                if bytes.len() as u64 >= FOLLOW_CHUNK {
                    // One line longer than a chunk carries no usage worth the memory: skip it.
                    self.offset += bytes.len() as u64;
                    continue;
                }
                break;
            };
            let mut line_start = self.offset;
            for line in bytes[..=end].split_inclusive(|byte| *byte == b'\n') {
                let at = line_start;
                line_start += line.len() as u64;
                let Some(row) = message_row(line, prices, self.sidechain) else {
                    continue;
                };
                match self
                    .pending
                    .iter_mut()
                    .find(|pending| pending.message.message_id == row.id)
                {
                    Some(pending) => {
                        pending.complete |= row.complete;
                        // The latest snapshot is authoritative; a stale row cannot lower it.
                        if row.timestamp >= pending.message.timestamp {
                            pending.message.timestamp = row.timestamp;
                            pending.message.usage = Usage {
                                cost_usd: row.cost,
                                ..row.usage
                            };
                            pending.message.categories = row.categories;
                        }
                    }
                    None => self.pending.push(PendingMessage {
                        first_offset: at,
                        complete: row.complete,
                        message: MessageUsage {
                            message_id: row.id,
                            timestamp: row.timestamp,
                            usage: Usage {
                                cost_usd: row.cost,
                                ..row.usage
                            },
                            categories: row.categories,
                        },
                    }),
                }
            }
            self.offset += end as u64 + 1;
        }
        Ok(())
    }

    /// The final messages, oldest first: every pending one but the newest, all of them when
    /// `turn_ended` or when the newest already named its stop reason. They stay pending
    /// until [`Self::recorded`] confirms them.
    pub fn ready(&self, turn_ended: bool) -> Vec<MessageUsage> {
        let newest_complete = self.pending.last().is_some_and(|pending| pending.complete);
        let count = if turn_ended || newest_complete {
            self.pending.len()
        } else {
            self.pending.len().saturating_sub(1)
        };
        self.pending[..count]
            .iter()
            .map(|pending| pending.message.clone())
            .collect()
    }

    /// The first `count` messages from [`Self::ready`] were recorded.
    pub fn recorded(&mut self, count: usize) {
        self.pending.drain(..count.min(self.pending.len()));
    }
}

/// A live notification-driven follow of one transcript's directory. Dropping it stops the
/// watcher thread.
pub struct TranscriptWatch {
    wake: std::sync::mpsc::Sender<bool>,
}

impl Drop for TranscriptWatch {
    fn drop(&mut self) {
        let _ = self.wake.send(false);
    }
}

/// How long after subscribing the watcher reads once more, because a write made before the
/// native stream started is never reported to it. One read, never a repeating timer.
const WATCH_SETTLE: std::time::Duration = std::time::Duration::from_secs(3);

/// The most subagent transcripts one session follows; more are ignored, never guessed.
const MAX_SUBAGENTS: usize = 1024;

/// The directory where Claude Code keeps a session's own files, beside its transcript:
/// `<projects>/<project>/<session>/`.
fn session_dir(transcript: &Path) -> std::path::PathBuf {
    transcript.with_extension("")
}

/// The background subagent transcripts of the session whose main transcript is
/// `transcript`, as `(agent id, path)` sorted by id: regular files named
/// `<session>/subagents/agent-<id>.jsonl` with a plain id (HS2-JW6N67). Symlinks and other
/// names are skipped; a missing directory has none.
pub fn subagent_transcripts(transcript: &Path) -> Vec<(String, std::path::PathBuf)> {
    let Ok(entries) = std::fs::read_dir(session_dir(transcript).join("subagents")) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let id = name
                .to_str()?
                .strip_prefix("agent-")?
                .strip_suffix(".jsonl")?;
            let plain = !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
            let regular = entry.file_type().ok()?.is_file();
            (plain && regular).then(|| (id.to_owned(), entry.path()))
        })
        .collect();
    found.sort();
    found.truncate(MAX_SUBAGENTS);
    found
}

/// Call `on_change` from a background thread whenever `transcript`'s directory or its
/// session's own directory (where subagent transcripts live, HS2-JW6N67) changes, once
/// right away, once more when the native watch is live, and once more after
/// [`WATCH_SETTLE`]. Wakes come from filesystem notifications and are coalesced; nothing
/// polls while the watch is live. The transcript's directory must exist.
///
/// HS2-PTK0CK: under load the macOS file-event service took seconds to start a stream and
/// up to tens of seconds to restart one, while it still delivered events within
/// milliseconds. So no wake waits behind a stream start or restart:
/// - the first read happens before the stream starts;
/// - on macOS one stream covers the transcript directory recursively from the start,
///   filtered to the transcript directory's own entries and this session's directory, so
///   the session directory created later never restarts the stream (elsewhere, where adding
///   a path restarts nothing, the session directory is still added once it exists);
/// - a stream the platform refuses is retried with a bounded backoff
///   (100 ms doubling to 5 s, by the shared `BackgroundWatcher`), reading at each step, instead
///   of ending the watch silently.
pub fn watch_transcript(
    transcript: &Path,
    on_change: impl FnMut() + Send + 'static,
) -> io::Result<TranscriptWatch> {
    watch_transcript_with(
        transcript,
        on_change,
        Box::new(hotsheet_jobs::watch::PathWatcher::start),
    )
}

/// [`watch_transcript`] with the native stream started by `start` (HS2-F6F42W: the shared
/// [`hotsheet_jobs::watch::BackgroundWatcher`] owns starting, retrying and stopping it).
fn watch_transcript_with(
    transcript: &Path,
    mut on_change: impl FnMut() + Send + 'static,
    start: hotsheet_jobs::watch::StartStream,
) -> io::Result<TranscriptWatch> {
    use hotsheet_jobs::watch::{BackgroundWatcher, Delivery, EventKind, RecursiveMode};
    use std::sync::mpsc::{RecvTimeoutError, channel};
    let dir = transcript
        .parent()
        .filter(|dir| dir.is_dir())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no transcript directory"))?
        .to_path_buf();
    let session = session_dir(transcript);
    let (wake, woken) = channel::<bool>();
    // HS2-PTK0CK: on macOS the stream is recursive from the start, so only the transcript
    // directory's own entries and this session's directory wake the follower.
    let recursive_from_start = cfg!(target_os = "macos");
    // The platform reports resolved paths (`/private/var/...` for `/var/...`), so both the
    // given and the resolved forms are matched.
    let resolved = hotsheet_sync::paths::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
    let forms: Vec<(std::path::PathBuf, std::path::PathBuf)> = [dir.clone(), resolved]
        .into_iter()
        .map(|form| {
            let session = session
                .file_name()
                .map_or_else(|| session.clone(), |name| form.join(name));
            (form, session)
        })
        .collect();
    let wanted = move |path: &Path| {
        !recursive_from_start
            || forms.iter().any(|(dir, session)| {
                path == dir || path.parent() == Some(dir.as_path()) || path.starts_with(session)
            })
    };
    let notify_wake = wake.clone();
    let handler = move |event: Delivery| {
        // Our own reads must not wake us again. A rescan notice names no path.
        if event.is_ok_and(|event| {
            !matches!(event.kind, EventKind::Access(_))
                && (event.paths.is_empty() || event.paths.iter().any(|path| wanted(path)))
        }) {
            let _ = notify_wake.send(true);
        }
    };
    // Going live, and each refused start while it is retried, is a read of its own.
    let status_wake = wake.clone();
    let status = move |_| {
        let _ = status_wake.send(true);
    };
    let mut session_watched = recursive_from_start || session.is_dir();
    let mut paths = vec![(
        dir.as_path(),
        if recursive_from_start {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        },
    )];
    if !recursive_from_start && session_watched {
        paths.push((session.as_path(), RecursiveMode::Recursive));
    }
    // Returns at once: the stream starts, retries and stops on its own thread.
    let watcher = BackgroundWatcher::start_with(&paths, handler, status, start);
    std::thread::spawn(move || {
        let watcher = watcher;
        // Right away: what is already there does not wait for the native stream.
        on_change();
        let mut settle = Some(std::time::Instant::now() + WATCH_SETTLE);
        loop {
            // HS2-F6F42W: until the stream is live nothing reports a change, so read again
            // at the re-check interval (local files only).
            if let Some(pending) = watcher.recheck() {
                let due = std::time::Instant::now() + pending;
                settle = Some(settle.map_or(due, |at| at.min(due)));
            }
            let next = match settle {
                Some(at) => match woken
                    .recv_timeout(at.saturating_duration_since(std::time::Instant::now()))
                {
                    Err(RecvTimeoutError::Timeout) => {
                        settle = None;
                        Ok(true)
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                    Ok(next) => Ok(next),
                },
                None => woken.recv(),
            };
            match next {
                Ok(true) => {
                    // One read covers a burst of notifications.
                    while let Ok(next) = woken.try_recv() {
                        if !next {
                            return;
                        }
                    }
                    // Claude creates the session directory on first use; its creation wakes
                    // the transcript directory's watch, and the session directory is added
                    // in the background (never on macOS, where the stream already covers it).
                    if !session_watched && session.is_dir() {
                        watcher.add(&session, RecursiveMode::Recursive);
                        session_watched = true;
                    }
                    on_change();
                }
                Ok(false) | Err(_) => return,
            }
        }
    });
    Ok(TranscriptWatch { wake })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn hs2_ssmdq1_follower_finalizes_each_message_once_a_later_one_appears() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", row("old", "1", 99, 0, 1, "claude-a")).unwrap();
        let start = file.as_file().metadata().unwrap().len();
        let mut follower = TranscriptFollower::new(start);
        // Empty, then a partial line that waits for its newline.
        follower.read_appended(file.path(), None).unwrap();
        assert!(!follower.has_pending());
        let a1 = row("a", "2026-01-01T00:00:01Z", 1, 2, 3, "claude-a");
        write!(file, "{}", &a1[..10]).unwrap();
        follower.read_appended(file.path(), None).unwrap();
        assert!(!follower.has_pending());
        writeln!(file, "{}", &a1[10..]).unwrap();
        writeln!(file, "not json").unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({"type":"assistant","isSidechain":true,"message":{"id":"sub","usage":{"input_tokens":100,"output_tokens":50}}})
        )
        .unwrap();
        // A repeated row with a later snapshot replaces the counters.
        writeln!(
            file,
            "{}",
            row("a", "2026-01-01T00:00:02Z", 1, 2, 9, "claude-a")
        )
        .unwrap();
        follower.read_appended(file.path(), None).unwrap();
        assert!(
            follower.ready(false).is_empty(),
            "the newest message may grow"
        );
        assert_eq!(follower.resume_offset(), start);
        writeln!(
            file,
            "{}",
            row("b", "2026-01-01T00:00:03Z", 4, 0, 5, "claude-a")
        )
        .unwrap();
        follower.read_appended(file.path(), None).unwrap();
        let ready = follower.ready(false);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].message_id, "a");
        assert_eq!(
            (ready[0].usage.tokens_in, ready[0].usage.tokens_out),
            (4, 9)
        );
        assert_eq!(ready[0].timestamp, "2026-01-01T00:00:02Z");
        follower.recorded(1);
        assert!(follower.resume_offset() > start);
        let ended = follower.ready(true);
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].message_id, "b");
        follower.recorded(1);
        assert!(!follower.has_pending());
        assert_eq!(
            follower.resume_offset(),
            file.as_file().metadata().unwrap().len()
        );

        // A replaced (shorter) transcript is read again from the start.
        std::fs::write(file.path(), format!("{}\n", row("c", "3", 1, 0, 1, "m"))).unwrap();
        follower.read_appended(file.path(), None).unwrap();
        assert_eq!(follower.ready(true)[0].message_id, "c");
        // A missing transcript reads nothing.
        let missing = file.path().with_extension("missing");
        TranscriptFollower::new(0)
            .read_appended(&missing, None)
            .unwrap();
    }

    #[test]
    fn hs2_ssmdq1_follower_prices_each_message_and_resumes_at_its_first_row() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let priced = serde_json::json!({
            "type": "assistant", "timestamp": "2026-01-01T00:00:01Z",
            "message": {"id": "priced", "model": "claude-fable-5-1", "usage": {
                "input_tokens": 10, "cache_read_input_tokens": 1000,
                "cache_creation_input_tokens": 30,
                "cache_creation": {"ephemeral_5m_input_tokens": 20, "ephemeral_1h_input_tokens": 10},
                "output_tokens": 5
            }}
        });
        writeln!(file, "{priced}").unwrap();
        let prices = hotsheet_ticketing::pricing::default_prices();
        let mut follower = TranscriptFollower::new(0);
        follower.read_appended(file.path(), Some(&prices)).unwrap();
        let resume = follower.resume_offset();
        assert_eq!(resume, 0);
        let ready = follower.ready(true);
        assert_eq!(ready[0].usage.cost_usd, Some(0.00105));
        assert!(ready[0].categories.is_some());
        // A fresh follower from the persisted resume point sees the same message.
        let mut resumed = TranscriptFollower::new(resume);
        resumed.read_appended(file.path(), Some(&prices)).unwrap();
        assert_eq!(resumed.ready(true), ready);
    }

    #[test]
    fn hs2_ssmdq1_transcript_watch_wakes_on_appends_until_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let (sent, received) = std::sync::mpsc::channel();
        let watch = watch_transcript(&path, move || {
            let _ = sent.send(());
        })
        .unwrap();
        received
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the first read runs at once");
        std::fs::write(&path, "x\n").unwrap();
        received
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("an append wakes the follower");
        drop(watch);
        assert!(watch_transcript(&dir.path().join("none/s.jsonl"), || {}).is_err());
    }

    fn agent_row(id: &str, at: &str, output: u64, stop: Option<&str>) -> String {
        serde_json::json!({
            "type": "assistant", "timestamp": at, "isSidechain": true, "agentId": "a1",
            "message": {
                "id": id, "model": "claude-a", "stop_reason": stop,
                "usage": {"input_tokens": 1, "output_tokens": output}
            }
        })
        .to_string()
    }

    #[test]
    fn hs2_jw6n67_subagent_follower_reads_only_sidechain_rows_and_finalizes_on_stop_reason() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        // A main-chain row never counts in a subagent transcript.
        writeln!(file, "{}", row("main", "1", 5, 0, 5, "claude-a")).unwrap();
        writeln!(file, "{}", agent_row("a", "2026-01-01T00:00:01Z", 2, None)).unwrap();
        let mut follower = TranscriptFollower::subagent(0);
        follower.read_appended(file.path(), None).unwrap();
        assert!(
            follower.ready(false).is_empty(),
            "the newest message may grow"
        );
        // Its stop-reason row carries the final counters: final with no later message.
        writeln!(
            file,
            "{}",
            agent_row("a", "2026-01-01T00:00:02Z", 7, Some("tool_use"))
        )
        .unwrap();
        follower.read_appended(file.path(), None).unwrap();
        let ready = follower.ready(false);
        assert_eq!(ready.len(), 1);
        assert_eq!(
            (ready[0].message_id.as_str(), ready[0].usage.tokens_out),
            ("a", 7)
        );
        follower.recorded(1);
        // A stale repeated row after the stop reason cannot reopen or lower it.
        writeln!(file, "{}", agent_row("b", "2026-01-01T00:00:03Z", 3, None)).unwrap();
        writeln!(
            file,
            "{}",
            agent_row("b", "2026-01-01T00:00:04Z", 4, Some("end_turn"))
        )
        .unwrap();
        writeln!(file, "{}", agent_row("b", "2026-01-01T00:00:03Z", 1, None)).unwrap();
        follower.read_appended(file.path(), None).unwrap();
        let ready = follower.ready(false);
        assert_eq!(
            (ready[0].message_id.as_str(), ready[0].usage.tokens_out),
            ("b", 4)
        );
        follower.recorded(1);
        assert!(!follower.has_pending());

        // The main follower still ignores every sidechain row, stop reason or not.
        let mut main = TranscriptFollower::new(0);
        main.read_appended(file.path(), None).unwrap();
        let main_ready = main.ready(true);
        assert_eq!(main_ready.len(), 1);
        assert_eq!(main_ready[0].message_id, "main");
    }

    #[test]
    fn hs2_jw6n67_lists_only_plain_regular_subagent_transcripts() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("session.jsonl");
        assert!(
            subagent_transcripts(&transcript).is_empty(),
            "no session directory"
        );
        let agents = dir.path().join("session").join("subagents");
        std::fs::create_dir_all(&agents).unwrap();
        for name in [
            "agent-b2.jsonl",
            "agent-a1.jsonl",
            "agent-a1.meta.json",
            "agent-.jsonl",
            "agent-x.y.jsonl",
            "other.jsonl",
        ] {
            std::fs::write(agents.join(name), "").unwrap();
        }
        std::fs::create_dir(agents.join("agent-dir.jsonl")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            agents.join("agent-a1.jsonl"),
            agents.join("agent-link.jsonl"),
        )
        .unwrap();
        let found = subagent_transcripts(&transcript);
        let ids: Vec<_> = found.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["a1", "b2"]);
        assert_eq!(found[0].1, agents.join("agent-a1.jsonl"));
    }

    /// HS2-PTK0CK: a watch whose native stream the platform refuses at first. The follower
    /// is woken right away, before any stream exists, and again at each bounded retry; once
    /// the stream starts, a change wakes it as usual. Before, a refused start ended the
    /// watch thread silently and its follower never woke (`a wake arrives: Timeout`).
    #[test]
    fn hs2_ptk0ck_a_refused_native_watch_still_wakes_its_follower_and_is_retried() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let attempts = std::sync::Arc::new(AtomicUsize::new(0));
        let counted = attempts.clone();
        let (sent, received) = std::sync::mpsc::channel();
        let (live, started) = std::sync::mpsc::channel();
        let mut start = starter_telling(live);
        let _watch = watch_transcript_with(
            &path,
            move || {
                let _ = sent.send(());
            },
            Box::new(move |paths, handler| {
                // Refused twice, as the platform did under load, then started for real.
                if counted.fetch_add(1, Ordering::SeqCst) < 2 {
                    return Err(hotsheet_jobs::watch::Error::generic(
                        "unable to start FSEvent stream",
                    ));
                }
                start(paths, handler)
            }),
        )
        .unwrap();
        // HS2-QCF0ZN: the first wake follows the native stream's start, which took longer than 10 s
        // under full-gate load; this test is about the wakes after it, so the bound only stops a
        // watch that never starts.
        let wait = || {
            received
                .recv_timeout(std::time::Duration::from_secs(30))
                .expect("a wake arrives");
        };
        // Right away, then once per refused attempt, without waiting for any stream.
        for _ in 0..3 {
            wait();
        }
        // HS2-F6F42W: the third attempt is the platform's own start; that it goes live and
        // reports events is the shared watcher's tested behavior (`hotsheet-jobs`
        // `hs2_f6f42w_a_refused_start_is_retried_with_backoff_until_it_starts`).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while attempts.load(Ordering::SeqCst) < 3 {
            assert!(
                std::time::Instant::now() < deadline,
                "the start was retried"
            );
            let _ = received.recv_timeout(std::time::Duration::from_millis(200));
        }
        drop(started);
    }

    /// HS2-PTK0CK: a stream start the platform holds up (seconds under load) does not hold
    /// up the first read: the follower is woken while the start is still blocked, and again
    /// once it returns.
    #[test]
    fn hs2_ptk0ck_the_first_read_never_waits_for_the_stream_start() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let (sent, received) = std::sync::mpsc::channel();
        let (entered, entering) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let released = std::sync::Mutex::new(released);
        let _watch = watch_transcript_with(
            &path,
            move || {
                let _ = sent.send(());
            },
            Box::new(move |paths, handler| {
                let _ = entered.send(());
                // Held until the test lets the platform answer.
                let _ = released.lock().unwrap().recv();
                hotsheet_jobs::watch::PathWatcher::start(paths, handler)
            }),
        )
        .unwrap();
        entering
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the start is attempted");
        received
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the first read does not wait for the start");
        assert!(
            received.try_recv().is_err(),
            "nothing else before the start"
        );
        release.send(()).unwrap();
        received
            .recv_timeout(STREAM_START)
            .expect("the live read follows the start");
    }

    /// HS2-PTK0CK: while the native stream stays refused, the follower keeps reading with a
    /// backoff that never shortens, and dropping the watch ends the retries.
    #[test]
    fn hs2_ptk0ck_a_watch_that_never_starts_backs_off_and_stops_when_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let attempts = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let counted = attempts.clone();
        let (sent, received) = std::sync::mpsc::channel();
        let watch = watch_transcript_with(
            &path,
            move || {
                let _ = sent.send(std::time::Instant::now());
            },
            Box::new(move |_paths, _handler| {
                counted.lock().unwrap().push(std::time::Instant::now());
                Err(hotsheet_jobs::watch::Error::generic("refused"))
            }),
        )
        .unwrap();
        // The follower keeps reading while the stream is refused.
        for _ in 0..5 {
            received
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("a degraded wake arrives");
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while attempts.lock().unwrap().len() < 4 {
            assert!(
                std::time::Instant::now() < deadline,
                "the start was retried"
            );
            let _ = received.recv_timeout(std::time::Duration::from_millis(200));
        }
        // Attempts 100, 200, 400 ms apart (bounded by START_RETRY_MAX): never a tighter loop.
        let tried = attempts.lock().unwrap().clone();
        for (pair, floor) in tried.windows(2).zip([100_u64, 200, 400]) {
            assert!(
                pair[1] - pair[0] >= std::time::Duration::from_millis(floor),
                "{:?} < {floor} ms",
                pair[1] - pair[0]
            );
        }
        drop(watch);
        let before = attempts.lock().unwrap().len();
        // The retry thread sees the drop on its next wait and ends; nothing else wakes.
        assert!(
            received
                .recv_timeout(
                    hotsheet_jobs::watch::START_RETRY_MAX + std::time::Duration::from_secs(2)
                )
                .is_err()
        );
        assert!(attempts.lock().unwrap().len() <= before + 1);
    }

    /// The platform watcher, telling the test once a stream is live: a stream start is the
    /// platform's own latency (seconds under load), which no wake waits behind any more, so
    /// a test that needs a live stream waits for exactly that (HS2-PTK0CK).
    fn starter_telling(live: std::sync::mpsc::Sender<()>) -> hotsheet_jobs::watch::StartStream {
        Box::new(move |paths, handler| {
            let started = hotsheet_jobs::watch::PathWatcher::start(paths, handler);
            if started.is_ok() {
                let _ = live.send(());
            }
            started
        })
    }

    /// How long a test lets the platform take to start one stream: its service, not ours,
    /// measured at up to 7 s on a loaded machine (HS2-PTK0CK) and past a minute while many
    /// processes started and stopped streams at once (HS2-F6F42W). No wake waits on it.
    const STREAM_START: std::time::Duration = std::time::Duration::from_secs(120);

    #[test]
    fn hs2_jw6n67_transcript_watch_wakes_on_subagent_appends_in_a_later_session_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let (sent, received) = std::sync::mpsc::channel();
        let (live, started) = std::sync::mpsc::channel();
        let _watch = watch_transcript_with(
            &path,
            move || {
                let _ = sent.send(());
            },
            starter_telling(live),
        )
        .unwrap();
        let wait = || {
            received
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("a wake arrives");
        };
        // HS2-PTK0CK: the first read never waits for the native stream.
        wait();
        started
            .recv_timeout(STREAM_START)
            .expect("the platform starts the native stream");
        // Its live read.
        wait();
        while received.try_recv().is_ok() {}
        // The session directory appears after the watch started; its creation wakes once,
        // and from then on appends deep inside it wake the follower too.
        let agents = dir.path().join("session").join("subagents");
        std::fs::create_dir_all(&agents).unwrap();
        wait();
        while received.try_recv().is_ok() {}
        let agent = agents.join("agent-a1.jsonl");
        // Retry the append until a wake proves the new directory is watched; a native stream
        // can miss writes made in the moment the watch is being added.
        for attempt in 0.. {
            std::fs::write(&agent, format!("{attempt}\n")).unwrap();
            if received
                .recv_timeout(std::time::Duration::from_secs(2))
                .is_ok()
            {
                break;
            }
            assert!(attempt < 10, "a subagent append never woke the follower");
        }
    }

    /// HS2-PTK0CK: on macOS one stream covers the transcript directory from the start, so a
    /// later session directory is followed without restarting the stream (a restart took up
    /// to tens of seconds under load), and only this session's files and the transcript
    /// directory's own entries wake the follower, never another session's directory.
    #[cfg(target_os = "macos")]
    #[test]
    fn hs2_ptk0ck_one_stream_covers_a_later_session_and_ignores_other_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let other = dir.path().join("other-session").join("subagents");
        std::fs::create_dir_all(&other).unwrap();
        let starts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = starts.clone();
        let (sent, received) = std::sync::mpsc::channel();
        let (live, started) = std::sync::mpsc::channel();
        let mut start = starter_telling(live);
        let _watch = watch_transcript_with(
            &path,
            move || {
                let _ = sent.send(());
            },
            Box::new(move |paths, handler| {
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                start(paths, handler)
            }),
        )
        .unwrap();
        started
            .recv_timeout(STREAM_START)
            .expect("the stream starts");
        let settled = || {
            // Every wake so far, including the settle read, has arrived.
            while received
                .recv_timeout(WATCH_SETTLE + std::time::Duration::from_secs(1))
                .is_ok()
            {}
        };
        settled();
        // Another session's subagent writes are not this follower's business.
        std::fs::write(other.join("agent-x.jsonl"), "x\n").unwrap();
        assert!(
            received
                .recv_timeout(std::time::Duration::from_secs(2))
                .is_err(),
            "another session's subagent woke this follower"
        );
        // This session's later directory and its subagents wake it with the same stream.
        let agents = dir.path().join("session").join("subagents");
        std::fs::create_dir_all(&agents).unwrap();
        received
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the session directory's creation wakes the follower");
        settled();
        std::fs::write(agents.join("agent-a1.jsonl"), "1\n").unwrap();
        received
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("a subagent append wakes the follower");
        assert_eq!(
            starts.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "one stream, never restarted"
        );
    }

    fn row(id: &str, at: &str, input: u64, cached: u64, output: u64, model: &str) -> String {
        serde_json::json!({
            "type": "assistant", "timestamp": at, "isSidechain": false,
            "message": {
                "id": id, "model": model,
                "usage": {
                    "input_tokens": input, "cache_read_input_tokens": cached,
                    "cache_creation_input_tokens": 1, "output_tokens": output
                }
            }
        })
        .to_string()
    }

    #[test]
    fn reads_only_new_turn_and_counts_each_message_once() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            row("old", "2026-01-01T00:00:00Z", 99, 0, 1, "old")
        )
        .unwrap();
        let offset = file.as_file().metadata().unwrap().len();
        writeln!(
            file,
            "{}",
            row("a", "2026-01-01T00:00:01Z", 2, 7, 3, "claude-a")
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            row("a", "2026-01-01T00:00:02Z", 2, 7, 3, "claude-a")
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            row("b", "2026-01-01T00:00:04Z", 4, 8, 5, "claude-a")
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            row("b", "2026-01-01T00:00:03Z", 1, 0, 1, "claude-a")
        )
        .unwrap();
        let usage = usage_since(file.path(), offset).unwrap().unwrap();
        assert_eq!((usage.tokens_in, usage.tokens_out), (23, 8));
        assert_eq!(usage.model.as_deref(), Some("claude-a"));
        assert_eq!(usage.cost_usd, None);
    }

    #[test]
    fn ignores_malformed_and_subagent_rows_and_handles_empty_then_refill() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let offset = file.as_file().metadata().unwrap().len();
        assert_eq!(usage_since(file.path(), offset).unwrap(), None);
        writeln!(file, "not json").unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({"type":"assistant","isSidechain":true,"message":{"id":"sub","usage":{"input_tokens":100,"output_tokens":50}}})
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            row("main", "2026-01-01T00:00:01Z", 1, 2, 3, "claude-b")
        )
        .unwrap();
        let usage = usage_since(file.path(), offset).unwrap().unwrap();
        assert_eq!((usage.tokens_in, usage.tokens_out), (4, 3));
        assert_eq!(usage.model.as_deref(), Some("claude-b"));
    }

    #[test]
    fn mixed_models_stay_unpriced_and_truncated_file_is_rejected() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", row("a", "1", 1, 0, 1, "claude-a")).unwrap();
        writeln!(file, "{}", row("b", "2", 1, 0, 1, "claude-b")).unwrap();
        let usage = usage_since(file.path(), 0).unwrap().unwrap();
        assert_eq!(usage.model, None);
        assert!(usage_since(file.path(), file.as_file().metadata().unwrap().len() + 1).is_err());
    }

    #[test]
    fn prices_deduplicated_cache_categories_without_charging_all_input_at_base_rate() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let row = serde_json::json!({
            "type": "assistant", "timestamp": "2026-01-01T00:00:01Z",
            "message": {"id": "priced", "model": "claude-fable-5-1", "usage": {
                "input_tokens": 10, "cache_read_input_tokens": 1000,
                "cache_creation_input_tokens": 30,
                "cache_creation": {"ephemeral_5m_input_tokens": 20, "ephemeral_1h_input_tokens": 10},
                "output_tokens": 5
            }}
        });
        writeln!(file, "{row}").unwrap();
        writeln!(file, "{row}").unwrap();
        let usage = priced_usage_since(
            file.path(),
            0,
            &hotsheet_ticketing::pricing::default_prices(),
        )
        .unwrap()
        .unwrap();
        assert_eq!((usage.tokens_in, usage.tokens_out), (1040, 5));
        // 10 × $10 + 1000 × $0.25 + 20 × $12.50 + 10 × $20 + 5 × $50, per million.
        assert_eq!(usage.cost_usd, Some(0.00105));
    }

    #[test]
    fn hs2_wz986q_keeps_latest_numeric_categories_and_unclassified_writes() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let newer = serde_json::json!({
            "type":"assistant", "timestamp":"2026-01-01T00:00:02Z",
            "message":{"id":"same", "model":"claude-fable-5-1", "usage":{
                "input_tokens":5, "cache_read_input_tokens":11,
                "cache_creation_input_tokens":7,
                "cache_creation":{"ephemeral_5m_input_tokens":2},
                "output_tokens":3
            }}
        });
        let older = serde_json::json!({
            "type":"assistant", "timestamp":"2026-01-01T00:00:01Z",
            "message":{"id":"same", "model":"claude-fable-5-1", "usage":{
                "input_tokens":1, "cache_read_input_tokens":1,
                "cache_creation_input_tokens":0, "output_tokens":1
            }}
        });
        writeln!(file, "{newer}\n{older}\n{newer}").unwrap();
        let turn = priced_usage_with_evidence_since(
            file.path(),
            0,
            &hotsheet_ticketing::pricing::default_prices(),
        )
        .unwrap()
        .unwrap();
        assert_eq!((turn.usage.tokens_in, turn.usage.tokens_out), (23, 3));
        assert_eq!(turn.usage.model.as_deref(), Some("claude-fable-5-1"));
        assert_eq!(
            turn.usage.cost_usd, None,
            "unknown write TTL cannot be priced"
        );
        assert_eq!(
            turn.categories,
            Some(ClaudeTokenCategories {
                uncached_input: 5,
                cache_read: 11,
                cache_write_5m: 2,
                cache_write_1h: 0,
                cache_write_unclassified: 5,
                output: 3,
            })
        );
    }

    #[test]
    fn hs2_wz986q_mixed_or_incomplete_models_have_no_repricing_evidence() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", row("a", "1", 2, 3, 4, "claude-fable-5-1")).unwrap();
        writeln!(file, "{}", row("b", "2", 5, 6, 7, "claude-opus-4-8")).unwrap();
        let turn = priced_usage_with_evidence_since(
            file.path(),
            0,
            &hotsheet_ticketing::pricing::default_prices(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(turn.usage.model, None);
        assert_eq!(turn.categories, None);

        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type":"assistant", "message":{"id":"a", "model":"claude-fable-5-1",
                "usage":{"input_tokens":2, "output_tokens":1}}
            })
        )
        .unwrap();
        let turn = priced_usage_with_evidence_since(
            file.path(),
            0,
            &hotsheet_ticketing::pricing::default_prices(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(turn.usage.model.as_deref(), Some("claude-fable-5-1"));
        assert_eq!(
            turn.categories, None,
            "missing cache counters are not guessed"
        );
        assert_eq!(
            turn.usage.cost_usd,
            Some(0.00007),
            "sparse rows retain initial pricing"
        );
    }
}
