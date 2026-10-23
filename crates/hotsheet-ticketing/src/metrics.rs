//! Usage/cost **metrics** storage (`docs/14`, HS2-69) — files, no DB. Every AI plugin's
//! `metrics` capability maps its tool's telemetry into a common [`UsageEvent`]; the host
//! appends those to **rotating raw JSONL** (`metrics/raw/<YYYY-MM-DD>.jsonl`, append-only)
//! and answers queries by aggregating them into a [`Rollup`].
//!
//! The raw JSONL is per-device (gitignored); the small **rollup files** are the shareable
//! unit, sharded **per contributor** (`metrics/rollups/<git-email>/rollup.json`) so two
//! people's files never conflict and a team view sums across them (`docs/14` §14.4). The
//! read path is DB-free and O(tail): [`summary_settled`] loads the settled rollup + live-
//! scans only the raw newer than `last_rolled_up_through`; [`roll_up_through`] advances that
//! frontier and [`prune_raw_before`] bounds the raw once settled. Rollups are a **derived
//! cache** — delete one and [`roll_up_through`] rebuilds it from the raw JSONL. The per-plugin
//! **telemetry mappers** (`docs/14` §14.7, tool → `UsageEvent`) are wired for codex
//! (`aitools::codex_turn_usage`, HS2-8PSAFE) and claude (`aitools::claude_result_usage`,
//! HS2-TJ8FGR), recorded end-to-end via `TurnEvent::Usage` → `record_priced`; mappers for
//! further plugins are the remaining follow-on.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};

use hotsheet_model::{Status, Timestamp};
use serde::{Deserialize, Serialize};

use crate::FileLock;
use crate::store::{FsStore, StoreError};

pub mod codex_work;

/// Attribute a turn only when these session identities hold exactly one unexpired claim.
/// Incomplete listings are ambiguous too: a corrupt ticket could hide another claim.
pub fn active_ticket_for_workers(
    store: &FsStore,
    workers: &[&str],
    now: &Timestamp,
) -> Result<Option<String>, StoreError> {
    let listing = store.list_tickets_resilient()?;
    if !listing.corrupt.is_empty() {
        return Ok(None);
    }
    let mut held = listing.tickets.into_iter().filter(|ticket| {
        ticket.status == Status::Started
            && ticket
                .claimed_by
                .as_deref()
                .is_some_and(|holder| workers.contains(&holder))
            && !crate::ops::claim_available(ticket, now)
    });
    let first = held.next().map(|ticket| ticket.id.to_string());
    Ok(first.filter(|_| held.next().is_none()))
}

/// One usage event — the common shape every tool's telemetry maps into (`docs/14` §14.2).
/// Unknown/unreported fields are omitted; cost is preferred tool-reported (else computed by
/// the caller from a price table).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageEvent {
    /// RFC3339 timestamp (its date picks the raw file).
    pub ts: String,
    /// Plugin id (`claude`, `codex`, …).
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// The active ticket (ULID string) when attributable, else `None` (project-only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    /// Opaque session id (for de-dup).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

/// Numeric Claude transcript evidence for a single-model turn. This contains no transcript
/// content and is written only when every counted assistant message has reliable categories
/// (HS2-WZ986Q). Older raw events have no `claude_tokens` member and stay ineligible for
/// historical repricing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeTokenCategories {
    pub uncached_input: u64,
    pub cache_read: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_write_unclassified: u64,
    pub output: u64,
}

impl ClaudeTokenCategories {
    pub fn add(&mut self, other: Self) {
        self.uncached_input = self.uncached_input.saturating_add(other.uncached_input);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write_5m = self.cache_write_5m.saturating_add(other.cache_write_5m);
        self.cache_write_1h = self.cache_write_1h.saturating_add(other.cache_write_1h);
        self.cache_write_unclassified = self
            .cache_write_unclassified
            .saturating_add(other.cache_write_unclassified);
        self.output = self.output.saturating_add(other.output);
    }
}

impl UsageEvent {
    /// The `YYYY-MM-DD` day the event belongs to (from `ts`), or `"unknown"`.
    fn day(&self) -> &str {
        self.ts.get(..10).unwrap_or("unknown")
    }
}

/// Aggregated totals across a set of events (`docs/14` §14.3). Additive per key, so two
/// people's rollups sum cleanly.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Rollup {
    pub events: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_usd: f64,
    /// Cost by model.
    pub by_model: BTreeMap<String, f64>,
    /// Cost by ticket ULID (only attributed events).
    pub by_ticket: BTreeMap<String, f64>,
    /// Cost by `YYYY-MM-DD`.
    pub by_day: BTreeMap<String, f64>,
}

impl Rollup {
    /// Fold `other` into `self` — additive per key (`docs/14` §14.4). This is what makes
    /// rollups mergeable: settled history + the live tail, and two contributors' shared
    /// rollups, all sum cleanly with no clobbering.
    pub fn merge(&mut self, other: &Rollup) {
        self.events += other.events;
        self.tokens_in += other.tokens_in;
        self.tokens_out += other.tokens_out;
        self.cost_usd += other.cost_usd;
        for (k, v) in &other.by_model {
            *self.by_model.entry(k.clone()).or_default() += v;
        }
        for (k, v) in &other.by_ticket {
            *self.by_ticket.entry(k.clone()).or_default() += v;
        }
        for (k, v) in &other.by_day {
            *self.by_day.entry(k.clone()).or_default() += v;
        }
    }
}

/// Append a usage event to the store's raw JSONL for its day (`metrics/raw/<day>.jsonl`),
/// creating the directory and gitignoring `metrics/raw/` (raw is per-device; only the small
/// rollup files are shared, §14.4).
pub fn record(store: &FsStore, event: &UsageEvent) -> io::Result<()> {
    let dir = store.root().join("metrics").join("raw");
    fs::create_dir_all(&dir)?;
    let _lock = FileLock::acquire(
        fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(".usage.lock"))?,
    )?;
    ensure_gitignored(store, "metrics/raw/")?;
    let path = dir.join(format!("{}.jsonl", event.day()));
    let line = serde_json::to_string(event).map_err(io::Error::other)?;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{line}")?;
    Ok(())
}

/// Record a turn once even when its caller retries after a write error. The identity
/// lives in raw JSONL but is ignored by the ordinary [`UsageEvent`] reader (HS2-F8578J).
pub fn record_once(store: &FsStore, event: &UsageEvent, turn_id: &str) -> io::Result<()> {
    record_once_with_claude_tokens(store, event, turn_id, None)
}

/// Preserve Claude's numeric categories beside the existing stable `turn_id`. The ordinary
/// `UsageEvent` reader ignores these extra raw members, so old readers and old rows remain
/// compatible (HS2-WZ986Q).
pub fn record_once_with_claude_tokens(
    store: &FsStore,
    event: &UsageEvent,
    turn_id: &str,
    categories: Option<ClaudeTokenCategories>,
) -> io::Result<()> {
    let dir = store.root().join("metrics").join("raw");
    fs::create_dir_all(&dir)?;
    let _lock = FileLock::acquire(
        fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(".usage.lock"))?,
    )?;
    ensure_gitignored(store, "metrics/raw/")?;
    let path = dir.join(format!("{}.jsonl", event.day()));
    match fs::File::open(&path) {
        Ok(file) => {
            for line in BufReader::new(file).lines() {
                let line = line?;
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                if value.get("turn_id").and_then(|id| id.as_str()) == Some(turn_id) {
                    return Ok(());
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut value = serde_json::to_value(event).map_err(io::Error::other)?;
    let object = value
        .as_object_mut()
        .expect("usage event serializes as an object");
    object.insert("turn_id".into(), turn_id.into());
    if let Some(categories) = categories {
        object.insert(
            "claude_tokens".into(),
            serde_json::to_value(categories).map_err(io::Error::other)?,
        );
    }
    let mut line = serde_json::to_vec(&value).map_err(io::Error::other)?;
    line.push(b'\n');
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(&line)
}

/// Read every raw usage event in the store (all days), skipping unparseable lines.
pub fn read_raw(store: &FsStore) -> io::Result<Vec<UsageEvent>> {
    let dir = store.root().join("metrics").join("raw");
    let mut events = Vec::new();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(events); // no metrics yet
    };
    let mut files: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
        .collect();
    files.sort();
    for path in files {
        let text = fs::read_to_string(&path)?;
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            if let Ok(ev) = serde_json::from_str::<UsageEvent>(line) {
                events.push(ev);
            }
        }
    }
    Ok(events)
}

/// Aggregate events into a [`Rollup`] (pure).
pub fn rollup(events: &[UsageEvent]) -> Rollup {
    let mut r = Rollup::default();
    for e in events {
        r.events += 1;
        r.tokens_in += e.tokens_in;
        r.tokens_out += e.tokens_out;
        let cost = e.cost_usd.unwrap_or(0.0);
        r.cost_usd += cost;
        if let Some(m) = &e.model {
            *r.by_model.entry(m.clone()).or_default() += cost;
        }
        if let Some(t) = &e.ticket {
            *r.by_ticket.entry(t.clone()).or_default() += cost;
        }
        *r.by_day.entry(e.day().to_string()).or_default() += cost;
    }
    r
}

/// Record a usage event, filling `cost_usd` from the store's price table (`docs/14` §14.2)
/// when the tool didn't report a cost and the model is priced — so cost is always present.
pub fn price_event(store: &FsStore, mut event: UsageEvent) -> UsageEvent {
    if event.cost_usd.is_none() {
        if let Some(model) = &event.model {
            let prices = crate::pricing::load_prices(store);
            event.cost_usd =
                crate::pricing::cost(&prices, model, event.tokens_in, event.tokens_out);
        }
    }
    event
}

pub fn record_priced(store: &FsStore, event: UsageEvent) -> io::Result<()> {
    record(store, &price_event(store, event))
}

/// The store's usage rollup — read the raw JSONL and aggregate (the DB-free read path).
pub fn summary(store: &FsStore) -> io::Result<Rollup> {
    Ok(rollup(&read_raw(store)?))
}

// ---- rollup files + settled read path (docs/14 §14.3/§14.4, HS2-8BCRHS) -----------

/// A contributor's settled rollup + how far it's been rolled up. Serialized to
/// `metrics/rollups/<contributor>/rollup.json` — the small, shareable unit (§14.4).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SettledRollup {
    /// The last day (`YYYY-MM-DD`) folded into `total`; raw newer than this is the live tail.
    #[serde(default)]
    last_rolled_up_through: String,
    #[serde(flatten)]
    total: Rollup,
}

/// The `metrics/rollups` directory.
fn rollups_dir(store: &FsStore) -> std::path::PathBuf {
    store.root().join("metrics").join("rollups")
}

/// A URL/path-safe contributor id: the store's git `user.email` (non-alphanumerics folded
/// to `-`), or `"local"` when git has no identity. Rollups shard by this so two people's
/// files never conflict and a team view sums across them (`docs/14` §14.4).
pub fn contributor_id(store: &FsStore) -> String {
    let email = crate::git::command()
        .arg("-C")
        .arg(store.root())
        .args(["config", "user.email"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    sanitize_contributor(&email)
}

fn sanitize_contributor(email: &str) -> String {
    if email.is_empty() {
        return "local".to_string();
    }
    email
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn read_settled(store: &FsStore, contributor: &str) -> SettledRollup {
    let path = rollups_dir(store).join(contributor).join("rollup.json");
    fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Fold every raw event **settled** as of `through_day` (`YYYY-MM-DD`) into this
/// contributor's rollup file, advancing `last_rolled_up_through` (`docs/14` §14.3). Only the
/// newly-settled window `(last_rolled_up_through, through_day]` is aggregated and **merged**
/// into the existing total, so the rollup survives raw-JSONL pruning (retention) and stays
/// correct across repeated runs. Returns the updated cumulative total.
pub fn roll_up_through(store: &FsStore, through_day: &str) -> io::Result<Rollup> {
    let contributor = contributor_id(store);
    let mut settled = read_settled(store, &contributor);
    // Aggregate only the tail beyond what's already settled, up to and including through_day.
    let tail: Vec<UsageEvent> = read_raw(store)?
        .into_iter()
        .filter(|e| e.day() > settled.last_rolled_up_through.as_str() && e.day() <= through_day)
        .collect();
    settled.total.merge(&rollup(&tail));
    settled.last_rolled_up_through = through_day.to_string();

    let dir = rollups_dir(store).join(&contributor);
    fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(&settled).map_err(io::Error::other)?;
    fs::write(dir.join("rollup.json"), json)?;
    Ok(settled.total.clone())
}

/// The DB-free read path (`docs/14` §14.3): this contributor's **settled** rollup file plus
/// a **live scan of only the raw tail** newer than `last_rolled_up_through`. Equivalent to
/// [`summary`] when nothing is rolled up yet (the whole store is "tail"), but O(tail) once a
/// roll-up has settled the history.
pub fn summary_settled(store: &FsStore) -> io::Result<Rollup> {
    let contributor = contributor_id(store);
    let settled = read_settled(store, &contributor);
    let mut total = settled.total;
    let tail: Vec<UsageEvent> = read_raw(store)?
        .into_iter()
        .filter(|e| e.day() > settled.last_rolled_up_through.as_str())
        .collect();
    total.merge(&rollup(&tail));
    Ok(total)
}

/// A **team** view: sum every contributor's settled rollup file (the shared unit that syncs
/// via git, §14.4) plus this device's own live raw tail. Cross-person, conflict-free.
pub fn team_summary(store: &FsStore) -> io::Result<Rollup> {
    let mut total = Rollup::default();
    if let Ok(entries) = fs::read_dir(rollups_dir(store)) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let name = entry.file_name().to_string_lossy().to_string();
                total.merge(&read_settled(store, &name).total);
            }
        }
    }
    // This device's un-rolled tail isn't in any rollup file yet — add it for a live view.
    let contributor = contributor_id(store);
    let settled = read_settled(store, &contributor);
    let tail: Vec<UsageEvent> = read_raw(store)?
        .into_iter()
        .filter(|e| e.day() > settled.last_rolled_up_through.as_str())
        .collect();
    total.merge(&rollup(&tail));
    Ok(total)
}

/// Retention (`docs/14` §14.3): delete raw JSONL files whose whole day is `< before_day` —
/// but **only** what's already rolled up (`before_day <= last_rolled_up_through`), so
/// un-settled events are never lost. Returns how many files were pruned.
pub fn prune_raw_before(store: &FsStore, before_day: &str) -> io::Result<usize> {
    let settled = read_settled(store, &contributor_id(store));
    // Never prune past the settled frontier — that data isn't in a rollup yet.
    if before_day > settled.last_rolled_up_through.as_str() {
        return Ok(0);
    }
    let dir = store.root().join("metrics").join("raw");
    let mut pruned = 0;
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(0);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let day = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if day < before_day {
            fs::remove_file(&path)?;
            pruned += 1;
        }
    }
    Ok(pruned)
}

/// One ticket's usage history for the timeline (HS2-ZA7W6E, HS2-J14225): every raw event
/// attributed to the ticket in time order, plus what the settled rollup already knows about
/// it, so a client can show per-phase figures while raw events exist and still a total after
/// the raw tail has been pruned.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TicketUsage {
    /// Attributed raw events in `ts` order (de-duplicated by `ts`+`session`+tokens).
    pub events: Vec<UsageEvent>,
    /// Cost of this ticket's events already folded into this contributor's settled rollup.
    /// Those events may no longer exist as raw lines when they are older than the retention
    /// window, so `events` can be a subset of what this cost covers.
    pub settled_cost_usd: f64,
    /// The last `YYYY-MM-DD` day folded into the settled rollup (`""` when none).
    pub settled_through: String,
}

/// Read one ticket's usage history (`docs/14` §14.5): raw attributed events in time order plus
/// the settled per-ticket cost. Unattributed events never match, and an unknown ticket yields
/// an empty history rather than an error.
pub fn ticket_usage(store: &FsStore, ticket: &str) -> io::Result<TicketUsage> {
    let mut events: Vec<UsageEvent> = read_raw(store)?
        .into_iter()
        .filter(|event| event.ticket.as_deref() == Some(ticket))
        .collect();
    events.sort_by(|left, right| left.ts.cmp(&right.ts));
    events.dedup_by(|left, right| {
        left.ts == right.ts
            && left.session == right.session
            && left.tool == right.tool
            && left.tokens_in == right.tokens_in
            && left.tokens_out == right.tokens_out
    });
    let settled = read_settled(store, &contributor_id(store));
    Ok(TicketUsage {
        events,
        settled_cost_usd: settled.total.by_ticket.get(ticket).copied().unwrap_or(0.0),
        settled_through: settled.last_rolled_up_through,
    })
}

/// Add `name` to the store's `.gitignore` if absent (raw metrics are per-device).
fn ensure_gitignored(store: &FsStore, name: &str) -> io::Result<()> {
    let gi = store.root().join(".gitignore");
    let existing = fs::read_to_string(&gi).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == name) {
        return Ok(());
    }
    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(name);
    out.push('\n');
    fs::write(&gi, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{self, NewTicket};
    use crate::store::StoreMetadata;
    use hotsheet_model::Ulid;

    fn store() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        (dir, store)
    }

    #[test]
    fn hs2_f8578j_raw_turn_identity_prevents_duplicate_usage() {
        let (_dir, store) = store();
        let event = UsageEvent {
            ts: "2026-10-23T00:00:00Z".into(),
            tool: "claude".into(),
            model: None,
            tokens_in: 7,
            tokens_out: 3,
            cost_usd: None,
            ticket: None,
            session: Some("session-1".into()),
        };
        record_once(&store, &event, "turn-1").unwrap();
        record_once(&store, &event, "turn-1").unwrap();
        assert_eq!(read_raw(&store).unwrap(), vec![event.clone()]);
        record_once(&store, &event, "turn-2").unwrap();
        assert_eq!(read_raw(&store).unwrap(), vec![event.clone(), event]);
    }

    #[test]
    fn hs2_wz986q_numeric_claude_evidence_preserves_old_raw_reader() {
        let (_dir, store) = store();
        let event = UsageEvent {
            ts: "2026-10-23T00:00:00Z".into(),
            tool: "claude".into(),
            model: Some("claude-fable-5-1".into()),
            tokens_in: 20,
            tokens_out: 2,
            cost_usd: None,
            ticket: None,
            session: Some("session-1".into()),
        };
        let categories = ClaudeTokenCategories {
            uncached_input: 5,
            cache_read: 10,
            cache_write_5m: 2,
            cache_write_1h: 1,
            cache_write_unclassified: 2,
            output: 2,
        };
        record_once_with_claude_tokens(&store, &event, "turn-1", Some(categories)).unwrap();
        record_once_with_claude_tokens(&store, &event, "turn-1", Some(categories)).unwrap();
        assert_eq!(read_raw(&store).unwrap(), vec![event]);
        let raw = fs::read_to_string(store.root().join("metrics/raw/2026-10-23.jsonl")).unwrap();
        let lines: Vec<_> = raw.lines().collect();
        assert_eq!(lines.len(), 1);
        let value: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(value["turn_id"], "turn-1");
        assert_eq!(
            serde_json::from_value::<ClaudeTokenCategories>(value["claude_tokens"].clone())
                .unwrap(),
            categories
        );
        assert_eq!(value["model"], "claude-fable-5-1");
    }

    #[test]
    fn usage_attribution_requires_one_live_claim_and_tracks_release_and_expiry() {
        let (_dir, store) = store();
        let first = ops::create(
            &store,
            Ulid::new(),
            "HS",
            Timestamp::new("2026-08-19T00:00:00Z"),
            NewTicket {
                title: "first".into(),
                category: "task".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let second = ops::create(
            &store,
            Ulid::new(),
            "HS",
            Timestamp::new("2026-08-19T00:00:00Z"),
            NewTicket {
                title: "second".into(),
                category: "task".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let at = Timestamp::new("2026-08-19T01:00:00Z");
        let lease = Timestamp::new("2026-08-19T01:30:00Z");
        assert_eq!(
            active_ticket_for_workers(&store, &["codex-a"], &at).unwrap(),
            None
        );
        ops::claim(&store, &first.id, &at, lease.clone(), "codex-a", None).unwrap();
        assert_eq!(
            active_ticket_for_workers(&store, &["codex-a"], &at).unwrap(),
            Some(first.id.to_string())
        );
        assert_eq!(
            active_ticket_for_workers(&store, &["other"], &at).unwrap(),
            None
        );
        ops::claim(&store, &second.id, &at, lease.clone(), "codex-a", None).unwrap();
        assert_eq!(
            active_ticket_for_workers(&store, &["codex-a"], &at).unwrap(),
            None,
            "two claims are ambiguous"
        );
        ops::release(&store, &second.id, at.clone(), "codex-a", false).unwrap();
        assert_eq!(
            active_ticket_for_workers(&store, &["codex-a"], &at).unwrap(),
            Some(first.id.to_string())
        );
        assert_eq!(
            active_ticket_for_workers(&store, &["codex-a"], &lease).unwrap(),
            None,
            "expired lease is not active"
        );
        ops::release(&store, &first.id, at.clone(), "codex-a", false).unwrap();
        assert_eq!(
            active_ticket_for_workers(&store, &["codex-a"], &at).unwrap(),
            None
        );
    }

    fn ev(
        ts: &str,
        tool: &str,
        model: &str,
        tin: u64,
        tout: u64,
        cost: f64,
        ticket: Option<&str>,
    ) -> UsageEvent {
        UsageEvent {
            ts: ts.into(),
            tool: tool.into(),
            model: Some(model.into()),
            tokens_in: tin,
            tokens_out: tout,
            cost_usd: Some(cost),
            ticket: ticket.map(str::to_string),
            session: None,
        }
    }

    #[test]
    fn record_appends_by_day_gitignores_and_reads_back() {
        let (_d, store) = store();
        record(
            &store,
            &ev(
                "2026-08-19T10:00:00Z",
                "claude",
                "opus",
                100,
                20,
                0.10,
                Some("T1"),
            ),
        )
        .unwrap();
        record(
            &store,
            &ev("2026-08-19T11:00:00Z", "claude", "opus", 50, 10, 0.05, None),
        )
        .unwrap();
        record(
            &store,
            &ev(
                "2026-08-20T09:00:00Z",
                "codex",
                "gpt",
                200,
                40,
                0.20,
                Some("T1"),
            ),
        )
        .unwrap();

        // Two day files, gitignored raw dir.
        assert!(store.root().join("metrics/raw/2026-08-19.jsonl").is_file());
        assert!(store.root().join("metrics/raw/2026-08-20.jsonl").is_file());
        let gi = fs::read_to_string(store.root().join(".gitignore")).unwrap();
        assert!(gi.lines().any(|l| l.trim() == "metrics/raw/"));

        let events = read_raw(&store).unwrap();
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn rollup_aggregates_by_model_ticket_and_day() {
        let events = vec![
            ev(
                "2026-08-19T10:00:00Z",
                "claude",
                "opus",
                100,
                20,
                0.10,
                Some("T1"),
            ),
            ev("2026-08-19T11:00:00Z", "claude", "opus", 50, 10, 0.05, None),
            ev(
                "2026-08-20T09:00:00Z",
                "codex",
                "gpt",
                200,
                40,
                0.20,
                Some("T1"),
            ),
        ];
        let r = rollup(&events);
        assert_eq!(r.events, 3);
        assert_eq!(r.tokens_in, 350);
        assert_eq!(r.tokens_out, 70);
        assert!((r.cost_usd - 0.35).abs() < 1e-9);
        assert!((r.by_model["opus"] - 0.15).abs() < 1e-9);
        assert!((r.by_model["gpt"] - 0.20).abs() < 1e-9);
        assert!((r.by_ticket["T1"] - 0.30).abs() < 1e-9, "T1 = 0.10 + 0.20");
        assert!(
            !r.by_ticket.contains_key(""),
            "unattributed events don't get a ticket key"
        );
        assert!((r.by_day["2026-08-19"] - 0.15).abs() < 1e-9);
    }

    #[test]
    fn ticket_usage_orders_filters_and_keeps_settled_cost_after_pruning() {
        let (_d, store) = store();
        let t = "01TICKETAAAAAAAAAAAAAAAAAA";
        record(
            &store,
            &ev("2026-08-20T10:00:00Z", "claude", "opus", 5, 1, 0.5, Some(t)),
        )
        .unwrap();
        record(
            &store,
            &ev(
                "2026-08-19T09:00:00Z",
                "claude",
                "opus",
                10,
                2,
                1.0,
                Some(t),
            ),
        )
        .unwrap();
        record(
            &store,
            &ev(
                "2026-08-19T09:30:00Z",
                "codex",
                "gpt",
                7,
                3,
                0.2,
                Some("other"),
            ),
        )
        .unwrap();
        record(
            &store,
            &ev("2026-08-19T09:45:00Z", "codex", "gpt", 7, 3, 0.2, None),
        )
        .unwrap();
        // The same event written twice (a replayed stream) counts once.
        record(
            &store,
            &ev("2026-08-20T10:00:00Z", "claude", "opus", 5, 1, 0.5, Some(t)),
        )
        .unwrap();

        let usage = ticket_usage(&store, t).unwrap();
        assert_eq!(
            usage
                .events
                .iter()
                .map(|e| e.ts.as_str())
                .collect::<Vec<_>>(),
            ["2026-08-19T09:00:00Z", "2026-08-20T10:00:00Z"]
        );
        assert_eq!(usage.settled_cost_usd, 0.0);
        assert_eq!(usage.settled_through, "");
        assert!(ticket_usage(&store, "missing").unwrap().events.is_empty());

        // Roll up both days and prune the first: the early event is gone from raw, its cost stays.
        roll_up_through(&store, "2026-08-20").unwrap();
        prune_raw_before(&store, "2026-08-20").unwrap();
        let usage = ticket_usage(&store, t).unwrap();
        assert_eq!(usage.events.len(), 1);
        assert_eq!(usage.events[0].ts, "2026-08-20T10:00:00Z");
        assert!((usage.settled_cost_usd - 2.0).abs() < 1e-9, "{usage:?}");
        assert_eq!(usage.settled_through, "2026-08-20");
    }

    #[test]
    fn summary_of_an_empty_store_is_zero() {
        let (_d, store) = store();
        let r = summary(&store).unwrap();
        assert_eq!(r, Rollup::default());
    }

    #[test]
    fn merge_is_additive_per_key() {
        let mut a = rollup(&[ev(
            "2026-08-19T10:00:00Z",
            "claude",
            "opus",
            100,
            20,
            0.10,
            Some("T1"),
        )]);
        let b = rollup(&[ev(
            "2026-08-20T09:00:00Z",
            "codex",
            "gpt",
            200,
            40,
            0.20,
            Some("T1"),
        )]);
        a.merge(&b);
        assert_eq!(a.events, 2);
        assert_eq!(a.tokens_in, 300);
        assert!((a.cost_usd - 0.30).abs() < 1e-9);
        assert!((a.by_ticket["T1"] - 0.30).abs() < 1e-9, "same key sums");
        assert!((a.by_model["opus"] - 0.10).abs() < 1e-9);
        assert!((a.by_model["gpt"] - 0.20).abs() < 1e-9);
        assert_eq!(a.by_day.len(), 2, "distinct days both kept");
    }

    #[test]
    fn rollup_files_settle_history_and_the_read_path_adds_only_the_tail() {
        let (_d, store) = store();
        // Three days of usage.
        for (ts, cost) in [
            ("2026-08-18T10:00:00Z", 0.10),
            ("2026-08-19T10:00:00Z", 0.20),
            ("2026-08-20T10:00:00Z", 0.30),
        ] {
            record(&store, &ev(ts, "claude", "opus", 100, 20, cost, Some("T1"))).unwrap();
        }

        // Roll up through the 19th: the 18th + 19th settle; the 20th is still tail.
        let settled = roll_up_through(&store, "2026-08-19").unwrap();
        assert_eq!(settled.events, 2, "two settled days");
        assert!((settled.cost_usd - 0.30).abs() < 1e-9);
        // The rollup file exists under the contributor shard.
        let cid = contributor_id(&store);
        assert!(
            store
                .root()
                .join("metrics/rollups")
                .join(&cid)
                .join("rollup.json")
                .is_file()
        );

        // The settled read path = settled rollup + live tail (the 20th) — matches a full scan.
        let fast = summary_settled(&store).unwrap();
        let full = summary(&store).unwrap();
        assert_eq!(fast, full, "settled + tail equals a full raw aggregate");
        assert_eq!(fast.events, 3);

        // Rolling up again through the 20th is idempotent — the already-settled window is
        // NOT double-counted (only the fresh tail folds in).
        let settled2 = roll_up_through(&store, "2026-08-20").unwrap();
        assert_eq!(settled2.events, 3, "no double-count");
        assert!((settled2.cost_usd - 0.60).abs() < 1e-9);
        // Now nothing is tail; the read path equals the settled total.
        assert_eq!(summary_settled(&store).unwrap(), settled2);
    }

    #[test]
    fn retention_prunes_only_settled_raw_and_rollup_survives() {
        let (_d, store) = store();
        for (ts, cost) in [
            ("2026-08-18T10:00:00Z", 0.10),
            ("2026-08-20T10:00:00Z", 0.30),
        ] {
            record(&store, &ev(ts, "claude", "opus", 100, 20, cost, Some("T1"))).unwrap();
        }
        roll_up_through(&store, "2026-08-19").unwrap(); // settles only the 18th

        // Pruning past the settled frontier is refused (would drop un-rolled data).
        assert_eq!(
            prune_raw_before(&store, "2026-08-21").unwrap(),
            0,
            "won't prune tail"
        );
        // Pruning within the settled range removes the old raw file.
        assert_eq!(prune_raw_before(&store, "2026-08-19").unwrap(), 1);
        assert!(!store.root().join("metrics/raw/2026-08-18.jsonl").exists());
        assert!(
            store.root().join("metrics/raw/2026-08-20.jsonl").exists(),
            "tail kept"
        );

        // The settled total still includes the pruned day (the whole point of rollups).
        let fast = summary_settled(&store).unwrap();
        assert_eq!(fast.events, 2, "pruned raw day survives in the rollup");
        assert!((fast.cost_usd - 0.40).abs() < 1e-9);
    }

    #[test]
    fn team_summary_sums_across_contributor_shards() {
        let (_d, store) = store();
        // This device's own settled rollup (one event).
        record(
            &store,
            &ev(
                "2026-08-19T10:00:00Z",
                "claude",
                "opus",
                100,
                20,
                0.10,
                Some("T1"),
            ),
        )
        .unwrap();
        roll_up_through(&store, "2026-08-19").unwrap();

        // A teammate's shared rollup file (as it'd arrive over git), hand-written.
        let mate = rollups_dir(&store).join("teammate-example-com");
        fs::create_dir_all(&mate).unwrap();
        let their = SettledRollup {
            last_rolled_up_through: "2026-08-19".into(),
            total: rollup(&[ev(
                "2026-08-19T12:00:00Z",
                "codex",
                "gpt",
                200,
                40,
                0.20,
                Some("T2"),
            )]),
        };
        fs::write(
            mate.join("rollup.json"),
            serde_json::to_string(&their).unwrap(),
        )
        .unwrap();

        let team = team_summary(&store).unwrap();
        assert_eq!(team.events, 2, "both contributors summed");
        assert!((team.cost_usd - 0.30).abs() < 1e-9);
        assert!((team.by_ticket["T1"] - 0.10).abs() < 1e-9);
        assert!((team.by_ticket["T2"] - 0.20).abs() < 1e-9);
    }

    #[test]
    fn contributor_id_is_path_safe() {
        assert_eq!(sanitize_contributor(""), "local");
        assert_eq!(sanitize_contributor("a.b+c@ex.com"), "a-b-c-ex-com");
    }
}

#[cfg(test)]
mod priced_tests {
    use super::*;
    use crate::store::StoreMetadata;

    #[test]
    fn record_priced_fills_cost_from_the_table_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        // No cost reported, but the model is priced → cost is computed.
        record_priced(
            &store,
            UsageEvent {
                ts: "2026-08-19T10:00:00Z".into(),
                tool: "claude".into(),
                model: Some("claude-opus-4-8".into()),
                tokens_in: 1_000_000,
                tokens_out: 0,
                cost_usd: None,
                ticket: None,
                session: None,
            },
        )
        .unwrap();
        // A tool-reported cost is kept as-is (not recomputed).
        record_priced(
            &store,
            UsageEvent {
                ts: "2026-08-19T11:00:00Z".into(),
                tool: "claude".into(),
                model: Some("claude-opus-4-8".into()),
                tokens_in: 1_000_000,
                tokens_out: 0,
                cost_usd: Some(0.01),
                ticket: None,
                session: None,
            },
        )
        .unwrap();

        let r = summary(&store).unwrap();
        // opus input is $15/Mtok → the first event computes to 15.00; the second keeps 0.01.
        assert!(
            (r.cost_usd - 15.01).abs() < 1e-9,
            "computed + reported: {}",
            r.cost_usd
        );
    }
}
