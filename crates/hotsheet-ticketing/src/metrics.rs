//! Usage/cost **metrics** storage (`docs/14`, HS2-69) — files, no DB. Every AI plugin's
//! `metrics` capability maps its tool's telemetry into a common [`UsageEvent`]; the host
//! appends those to **rotating raw JSONL** (`metrics/raw/<YYYY-MM-DD>.jsonl`, append-only)
//! and answers queries by aggregating them into a [`Rollup`].
//!
//! Scope here is the storage + read path (raw writer, reader, aggregation). The raw JSONL
//! is per-device (gitignored); **team sharing of the small rollup files via git** and the
//! per-plugin mappers are follow-ons (`docs/14` §14.4/§14.7). Reads are DB-free: this is
//! the "derived cache" principle again — delete a rollup and it rebuilds from the raw JSONL.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};

use serde::{Deserialize, Serialize};

use crate::store::FsStore;

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

/// Append a usage event to the store's raw JSONL for its day (`metrics/raw/<day>.jsonl`),
/// creating the directory and gitignoring `metrics/raw/` (raw is per-device; only the small
/// rollup files are shared, §14.4).
pub fn record(store: &FsStore, event: &UsageEvent) -> io::Result<()> {
    let dir = store.root().join("metrics").join("raw");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.jsonl", event.day()));
    let line = serde_json::to_string(event).map_err(io::Error::other)?;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{line}")?;
    ensure_gitignored(store, "metrics/raw/")?;
    Ok(())
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

/// The store's usage rollup — read the raw JSONL and aggregate (the DB-free read path).
pub fn summary(store: &FsStore) -> io::Result<Rollup> {
    Ok(rollup(&read_raw(store)?))
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
    use crate::store::StoreMetadata;

    fn store() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        (dir, store)
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
    fn summary_of_an_empty_store_is_zero() {
        let (_d, store) = store();
        let r = summary(&store).unwrap();
        assert_eq!(r, Rollup::default());
    }
}
