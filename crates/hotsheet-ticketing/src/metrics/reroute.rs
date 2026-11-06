//! Move misrouted raw usage rows into the store they belong to (HS2-B4EQ87).
//!
//! Until HS2-B4EQ87 a machine server recorded every terminal turn in its bootstrap store.
//! [`reroute_raw_usage`] moves each row a caller's route assigns to another store: it appends
//! the unchanged raw line to that store's day file, then removes it from the source. The
//! append skips a row the target already holds (same `turn_id`, else the identical line), so
//! an interrupted or repeated run converges without duplicates. Rows on or before either
//! store's settled roll-up frontier stay where they are, because moving them would change a
//! settled total. Nothing is written unless `apply` is set.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::{UsageEvent, contributor_id, ensure_gitignored, read_settled, reprice};
use crate::FileLock;
use crate::store::FsStore;

/// What a reroute moved or would move, and what it kept.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RerouteReport {
    /// Rows moved (or, in a preview, to move) by target store root.
    pub moved: BTreeMap<PathBuf, usize>,
    /// Moved rows the target already held, so only the source copy was removed.
    pub already_in_target: usize,
    /// Rows the route assigned to no other store.
    pub unrouted: usize,
    /// Rows kept because their day is settled in the source or the target.
    pub settled: usize,
    /// Lines that are not usage events, kept unchanged.
    pub unreadable: usize,
}

impl RerouteReport {
    /// Total rows moved or to move.
    pub fn moved_total(&self) -> usize {
        self.moved.values().sum()
    }
}

/// One row the route assigned elsewhere.
struct Move {
    target: usize,
    line: String,
    turn_id: Option<String>,
}

fn raw_dir(store: &FsStore) -> PathBuf {
    store.root().join("metrics").join("raw")
}

fn lock_raw(store: &FsStore) -> io::Result<FileLock> {
    let dir = raw_dir(store);
    fs::create_dir_all(&dir)?;
    FileLock::acquire(
        fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(".usage.lock"))?,
    )
}

fn settled_through(store: &FsStore) -> io::Result<String> {
    Ok(read_settled(store, &contributor_id(store))?.last_rolled_up_through)
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Move every raw row of `source` that `route` assigns to another store. `route` sees the
/// raw row and its parsed event and returns the store it belongs in, or `None` to keep it.
/// With `apply` false nothing is written and the report says what would move.
pub fn reroute_raw_usage(
    source: &FsStore,
    mut route: impl FnMut(&serde_json::Value, &UsageEvent) -> Option<FsStore>,
    apply: bool,
) -> io::Result<RerouteReport> {
    let mut report = RerouteReport::default();
    let dir = raw_dir(source);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(report);
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
        .collect();
    files.sort();
    let _source_lock = if apply { Some(lock_raw(source)?) } else { None };
    let source_root = canonical(source.root());
    let source_settled = settled_through(source)?;
    // Targets by canonical root, with their settled frontier.
    let mut targets: Vec<(FsStore, PathBuf, String)> = Vec::new();
    for path in files {
        let text = fs::read_to_string(&path)?;
        let mut kept = Vec::new();
        let mut moves = Vec::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let parsed = serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|value| {
                    serde_json::from_value::<UsageEvent>(value.clone())
                        .ok()
                        .map(|event| (value, event))
                });
            let Some((value, event)) = parsed else {
                report.unreadable += 1;
                kept.push(line);
                continue;
            };
            let Some(target) = route(&value, &event) else {
                report.unrouted += 1;
                kept.push(line);
                continue;
            };
            let root = canonical(target.root());
            if root == source_root {
                report.unrouted += 1;
                kept.push(line);
                continue;
            }
            let index = match targets.iter().position(|(_, known, _)| *known == root) {
                Some(index) => index,
                None => {
                    let settled = settled_through(&target)?;
                    targets.push((target, root, settled));
                    targets.len() - 1
                }
            };
            let day = event.day();
            if day <= source_settled.as_str() || day <= targets[index].2.as_str() {
                report.settled += 1;
                kept.push(line);
                continue;
            }
            moves.push(Move {
                target: index,
                line: line.to_owned(),
                turn_id: value
                    .get("turn_id")
                    .and_then(|id| id.as_str())
                    .map(str::to_owned),
            });
        }
        if moves.is_empty() {
            continue;
        }
        let name = path.file_name().expect("raw day file has a name");
        for (index, (target, root, _)) in targets.iter().enumerate() {
            let rows: Vec<&Move> = moves.iter().filter(|row| row.target == index).collect();
            if rows.is_empty() {
                continue;
            }
            *report.moved.entry(root.clone()).or_default() += rows.len();
            if apply {
                report.already_in_target += append_missing(target, Path::new(name), &rows)?;
            }
        }
        if apply {
            // The target copies are durable first, so a stop here leaves duplicates that the
            // next run removes from the source without appending again.
            if kept.is_empty() {
                fs::remove_file(&path)?;
            } else {
                let mut bytes = kept.join("\n").into_bytes();
                bytes.push(b'\n');
                reprice::atomic_write(&path, &bytes)?;
            }
        }
    }
    Ok(report)
}

/// Append the rows `target` does not hold yet to its day file `name`. Returns how many it
/// already held.
fn append_missing(target: &FsStore, name: &Path, rows: &[&Move]) -> io::Result<usize> {
    let _lock = lock_raw(target)?;
    ensure_gitignored(target, "metrics/raw/")?;
    let path = raw_dir(target).join(name);
    let existing = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let mut lines: HashSet<String> = HashSet::new();
    let mut turns: HashSet<String> = HashSet::new();
    for line in existing.lines() {
        lines.insert(line.to_owned());
        if let Some(id) = serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|value| value.get("turn_id")?.as_str().map(str::to_owned))
        {
            turns.insert(id);
        }
    }
    let mut present = 0;
    let mut appended = String::new();
    for row in rows {
        let held = match &row.turn_id {
            Some(id) => !turns.insert(id.clone()),
            None => !lines.insert(row.line.clone()),
        };
        if held {
            present += 1;
            continue;
        }
        appended.push_str(&row.line);
        appended.push('\n');
    }
    if !appended.is_empty() {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        // A day file written by another tool may not end in a newline.
        if !existing.is_empty() && !existing.ends_with('\n') {
            file.write_all(b"\n")?;
        }
        file.write_all(appended.as_bytes())?;
        file.sync_all()?;
    }
    Ok(present)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoreMetadata;
    use crate::metrics::{read_raw, record, record_once, roll_up_through};

    fn event(ts: &str, session: &str, tokens: u64) -> UsageEvent {
        UsageEvent {
            ts: ts.into(),
            tool: "claude".into(),
            model: Some("claude-opus-4-8".into()),
            tokens_in: tokens,
            tokens_out: 1,
            cost_usd: None,
            ticket: None,
            session: Some(session.into()),
        }
    }

    fn stores() -> (tempfile::TempDir, FsStore, FsStore, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let open = |name: &str, prefix: &str| {
            FsStore::init(dir.path().join(name), &StoreMetadata::new(prefix)).unwrap()
        };
        let (source, a, b) = (
            open("boot.hs2", "BT"),
            open("a.hs2", "AA"),
            open("b.hs2", "BB"),
        );
        (dir, source, a, b)
    }

    fn sessions(store: &FsStore) -> Vec<String> {
        read_raw(store)
            .unwrap()
            .into_iter()
            .filter_map(|event| event.session)
            .collect()
    }

    /// HS2-B4EQ87: preview writes nothing; apply moves each routed row once and keeps the
    /// rest; a rerun moves nothing more.
    #[test]
    fn hs2_b4eq87_reroute_previews_moves_and_converges() {
        let (_dir, source, a, b) = stores();
        record_once(
            &source,
            &event("2026-11-01T00:00:00Z", "to-a", 10),
            "turn-1",
        )
        .unwrap();
        record(&source, &event("2026-11-01T00:01:00Z", "to-b", 20)).unwrap();
        record(&source, &event("2026-11-02T00:00:00Z", "stay", 30)).unwrap();
        record_once(
            &source,
            &event("2026-11-02T00:01:00Z", "to-a", 40),
            "turn-2",
        )
        .unwrap();
        let raw = source.root().join("metrics/raw/2026-11-02.jsonl");
        let mut file = fs::OpenOptions::new().append(true).open(&raw).unwrap();
        writeln!(file, "not json").unwrap();
        let routes = |(a, b): (&FsStore, &FsStore)| {
            let (a, b) = (a.clone(), b.clone());
            move |_: &serde_json::Value, event: &UsageEvent| match event.session.as_deref() {
                Some("to-a") => Some(a.clone()),
                Some("to-b") => Some(b.clone()),
                _ => None,
            }
        };

        let preview = reroute_raw_usage(&source, routes((&a, &b)), false).unwrap();
        assert_eq!(preview.moved_total(), 3);
        assert_eq!((preview.unrouted, preview.unreadable), (1, 1));
        assert_eq!(sessions(&source).len(), 4);
        assert!(read_raw(&a).unwrap().is_empty());

        let applied = reroute_raw_usage(&source, routes((&a, &b)), true).unwrap();
        assert_eq!(applied.moved, preview.moved);
        assert_eq!(applied.already_in_target, 0);
        assert_eq!(sessions(&source), ["stay"]);
        assert_eq!(sessions(&a), ["to-a", "to-a"]);
        assert_eq!(sessions(&b), ["to-b"]);
        assert!(!source.root().join("metrics/raw/2026-11-01.jsonl").exists());
        assert!(fs::read_to_string(&raw).unwrap().contains("not json"));
        // The moved row keeps its turn identity, so the target's own retry dedup holds.
        record_once(&a, &event("2026-11-02T00:01:00Z", "to-a", 40), "turn-2").unwrap();
        assert_eq!(sessions(&a).len(), 2);

        let again = reroute_raw_usage(&source, routes((&a, &b)), true).unwrap();
        assert_eq!(again.moved_total(), 0);
        assert_eq!(sessions(&a).len(), 2);
    }

    /// HS2-B4EQ87: a run stopped after appending to the target and before rewriting the
    /// source leaves both copies; the next run removes only the source copy.
    #[test]
    fn hs2_b4eq87_reroute_after_an_interrupted_run_does_not_duplicate() {
        let (_dir, source, a, _b) = stores();
        record_once(
            &source,
            &event("2026-11-01T00:00:00Z", "to-a", 10),
            "turn-1",
        )
        .unwrap();
        record(&source, &event("2026-11-01T00:01:00Z", "to-a", 20)).unwrap();
        let day = source.root().join("metrics/raw/2026-11-01.jsonl");
        fs::create_dir_all(a.root().join("metrics/raw")).unwrap();
        fs::copy(&day, a.root().join("metrics/raw/2026-11-01.jsonl")).unwrap();
        let target = a.clone();
        let report = reroute_raw_usage(&source, move |_, _| Some(target.clone()), true).unwrap();
        assert_eq!(report.moved_total(), 2);
        assert_eq!(report.already_in_target, 2);
        assert!(read_raw(&source).unwrap().is_empty());
        assert_eq!(read_raw(&a).unwrap().len(), 2);
    }

    /// HS2-B4EQ87: settled days stay in place in either direction; rows routed to the
    /// source itself are not moved.
    #[test]
    fn hs2_b4eq87_reroute_keeps_settled_days_and_self_routes() {
        let (_dir, source, a, b) = stores();
        record(&source, &event("2026-10-30T00:00:00Z", "to-a", 1)).unwrap();
        record(&source, &event("2026-10-31T00:00:00Z", "to-b", 2)).unwrap();
        record(&source, &event("2026-11-01T00:00:00Z", "to-b", 3)).unwrap();
        record(&source, &event("2026-11-01T00:00:01Z", "self", 4)).unwrap();
        roll_up_through(&source, "2026-10-30").unwrap();
        roll_up_through(&b, "2026-10-31").unwrap();
        let (to_a, to_b, own) = (a.clone(), b.clone(), source.clone());
        let report = reroute_raw_usage(
            &source,
            move |_, event| match event.session.as_deref() {
                Some("to-a") => Some(to_a.clone()),
                Some("to-b") => Some(to_b.clone()),
                _ => Some(own.clone()),
            },
            true,
        )
        .unwrap();
        assert_eq!(report.settled, 2);
        assert_eq!(report.unrouted, 1);
        assert_eq!(report.moved_total(), 1);
        assert_eq!(sessions(&b), ["to-b"]);
        assert_eq!(sessions(&source), ["to-a", "to-b", "self"]);
    }
}
