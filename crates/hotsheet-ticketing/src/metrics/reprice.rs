//! Explicit, local Claude usage corrections (HS2-38SAET). Raw JSONL is evidence and
//! never rewritten; the ledger is the atomic authority for its effective price.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ClaudeTokenCategories, Rollup, UsageEvent, ensure_gitignored};
use crate::FileLock;
use crate::pricing::{self, Price};
use crate::store::FsStore;

const LEDGER_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Correction {
    raw_sha256: String,
    price_revision: String,
    rate: Price,
    categories: ClaudeTokenCategories,
    cost_usd: f64,
    day: String,
    model: String,
    ticket: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Ledger {
    version: u32,
    entries: BTreeMap<String, Correction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepriceRow {
    pub event_id: String,
    pub model: String,
    pub price_revision: String,
    pub rate: Price,
    pub previous_cost_usd: f64,
    pub effective_cost_usd: f64,
    pub delta_usd: f64,
}

#[derive(Debug, Serialize)]
pub struct RepriceReport {
    pub rows: Vec<RepriceRow>,
    pub changed: usize,
}

struct RawEvidence {
    event: UsageEvent,
    id: Option<String>,
    categories: Option<ClaudeTokenCategories>,
    hash: String,
}

fn metrics_dir(store: &FsStore) -> PathBuf {
    store.root().join("metrics")
}

fn ledger_path(store: &FsStore) -> PathBuf {
    metrics_dir(store).join("local/corrections.json")
}

fn checkpoint_path(store: &FsStore, slot: u8) -> PathBuf {
    metrics_dir(store).join(format!("local/corrections.checkpoint-{slot}.json"))
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn load_ledger(store: &FsStore) -> io::Result<Ledger> {
    let text = match fs::read_to_string(ledger_path(store)) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let settled = super::read_settled(store, &super::contributor_id(store))?;
            if settled.checkpoint_sha256.is_some() {
                return Err(invalid(
                    "correction ledger missing after raw usage was pruned",
                ));
            }
            return Ok(Ledger::default());
        }
        Err(error) => return Err(error),
    };
    parse_ledger(&text)
}

fn parse_ledger(text: &str) -> io::Result<Ledger> {
    let ledger: Ledger = serde_json::from_str(text)
        .map_err(|error| invalid(format!("corrupt correction ledger: {error}")))?;
    if ledger.version != LEDGER_VERSION {
        return Err(invalid(format!(
            "unsupported correction ledger version {}",
            ledger.version
        )));
    }
    if ledger.entries.iter().any(|(id, entry)| {
        let expected_revision = serde_json::to_vec(&entry.rate)
            .ok()
            .map(|bytes| digest(&bytes));
        let expected_cost = entry.rate.claude_cost(
            entry.categories.uncached_input,
            entry.categories.cache_read,
            entry.categories.cache_write_5m,
            entry.categories.cache_write_1h,
            entry.categories.cache_write_unclassified,
            entry.categories.output,
        );
        id.is_empty()
            || entry.raw_sha256.len() != 64
            || !usable_rate(entry.rate)
            || expected_revision.as_deref() != Some(entry.price_revision.as_str())
            || expected_cost.is_none_or(|cost| (cost - entry.cost_usd).abs() > 1e-9)
            || !entry.cost_usd.is_finite()
            || entry.cost_usd < 0.0
            || entry.day.len() != 10
    }) {
        return Err(invalid("invalid correction ledger entry"));
    }
    Ok(ledger)
}

/// Persist the complete numeric correction state before retention deletes raw evidence.
/// The settled prune marker records its slot and exact byte hash. Write the
/// inactive slot so a crash before that marker commits leaves the prior
/// checkpoint available.
pub(super) fn checkpoint(store: &FsStore, slot: u8) -> io::Result<String> {
    let mut ledger = load_ledger(store)?;
    ledger.version = LEDGER_VERSION;
    let bytes = serde_json::to_vec_pretty(&ledger).map_err(io::Error::other)?;
    // A store with no eligible Claude corrections has no primary ledger yet.
    // Materialize the validated empty ledger so post-prune readers can still
    // distinguish that state from a lost primary file.
    if !ledger_path(store).exists() {
        atomic_write(&ledger_path(store), &bytes)?;
    }
    atomic_write(&checkpoint_path(store, slot), &bytes)?;
    Ok(digest(&bytes))
}

fn load_checkpoint(store: &FsStore, slot: u8, expected_sha256: &str) -> io::Result<Ledger> {
    if slot > 1 {
        return Err(invalid("invalid correction checkpoint slot"));
    }
    let bytes = fs::read(checkpoint_path(store, slot))?;
    if digest(&bytes) != expected_sha256 {
        return Err(invalid(
            "correction checkpoint does not match settled prune marker",
        ));
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| invalid("correction checkpoint is not UTF-8"))?;
    parse_ledger(text)
}

/// All correction-aware readers fail closed when the ledger is damaged.
pub(super) fn corrected_costs(store: &FsStore) -> io::Result<BTreeMap<String, Correction>> {
    Ok(load_ledger(store)?.entries)
}

pub(super) fn lock(store: &FsStore) -> io::Result<FileLock> {
    // Share the raw writer lock: apply, settlement, and retention see complete
    // append snapshots, and concurrent first-use .gitignore edits cannot race.
    let raw_dir = metrics_dir(store).join("raw");
    fs::create_dir_all(&raw_dir)?;
    let lock = FileLock::acquire(
        fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(raw_dir.join(".usage.lock"))?,
    )?;
    ensure_gitignored(store, "metrics/raw/")?;
    ensure_gitignored(store, "metrics/local/")?;
    fs::create_dir_all(metrics_dir(store).join("local"))?;
    Ok(lock)
}

pub(super) fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> io::Result<()> {
    let staged = path.with_extension(format!("{}.tmp", ulid::Ulid::new()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&staged, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(staged);
    }
    result
}

fn read_evidence(store: &FsStore) -> io::Result<Vec<RawEvidence>> {
    let dir = metrics_dir(store).join("raw");
    let mut result = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(result),
        Err(error) => return Err(error),
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        for line in BufReader::new(File::open(path)?).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let value: serde_json::Value = match serde_json::from_str(&line) {
                Ok(value) => value,
                Err(_) => continue, // Preserve the existing raw reader's tolerant behavior.
            };
            let Ok(event) = serde_json::from_value::<UsageEvent>(value.clone()) else {
                continue;
            };
            let id = value
                .get("turn_id")
                .and_then(|value| value.as_str())
                .map(str::to_owned);
            let categories = value
                .get("claude_tokens")
                .and_then(|value| serde_json::from_value(value.clone()).ok());
            result.push(RawEvidence {
                event,
                id,
                categories,
                hash: digest(line.as_bytes()),
            });
        }
    }
    Ok(result)
}

fn usable_rate(price: Price) -> bool {
    [
        Some(price.input_per_mtok),
        Some(price.output_per_mtok),
        price.cache_read_per_mtok,
        price.cache_write_5m_per_mtok,
        price.cache_write_1h_per_mtok,
    ]
    .into_iter()
    .flatten()
    .all(|rate| rate.is_finite() && rate >= 0.0)
}

fn effective_prices(store: &FsStore) -> io::Result<pricing::Prices> {
    let mut prices = pricing::default_prices();
    let path = metrics_dir(store).join("prices.json");
    match fs::read_to_string(path) {
        Ok(text) => {
            let overrides: pricing::Prices = serde_json::from_str(&text)
                .map_err(|error| invalid(format!("invalid price override: {error}")))?;
            prices.extend(overrides);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if prices.values().any(|price| !usable_rate(*price)) {
        return Err(invalid(
            "price table contains a negative or non-finite rate",
        ));
    }
    Ok(prices)
}

fn candidates(
    store: &FsStore,
    ledger: &Ledger,
) -> io::Result<(Vec<RepriceRow>, BTreeMap<String, Correction>)> {
    let prices = effective_prices(store)?;
    let mut rows = Vec::new();
    let mut next = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for raw in read_evidence(store)? {
        let Some(id) = raw.id else { continue };
        if !ids.insert(id.clone()) {
            return Err(invalid(format!("duplicate raw event identity: {id}")));
        }
        if let Some(previous) = ledger.entries.get(&id) {
            if previous.raw_sha256 != raw.hash {
                return Err(invalid(format!("raw evidence changed for event {id}")));
            }
        }
        let Some(categories) = raw.categories else {
            continue;
        };
        let counted_input = categories
            .uncached_input
            .checked_add(categories.cache_read)
            .and_then(|count| count.checked_add(categories.cache_write_5m))
            .and_then(|count| count.checked_add(categories.cache_write_1h))
            .and_then(|count| count.checked_add(categories.cache_write_unclassified));
        if counted_input != Some(raw.event.tokens_in) || categories.output != raw.event.tokens_out {
            continue;
        }
        let Some(model) = raw.event.model.as_deref() else {
            continue;
        };
        if raw.event.tool != "claude" || raw.event.cost_usd.is_some() || id.is_empty() {
            continue;
        }
        let Some(price) = prices
            .get(model)
            .copied()
            .filter(|price| usable_rate(*price))
        else {
            continue;
        };
        let Some(cost) = price.claude_cost(
            categories.uncached_input,
            categories.cache_read,
            categories.cache_write_5m,
            categories.cache_write_1h,
            categories.cache_write_unclassified,
            categories.output,
        ) else {
            continue;
        };
        if !cost.is_finite() || cost < 0.0 {
            continue;
        }
        let revision = digest(&serde_json::to_vec(&price).map_err(io::Error::other)?);
        let previous_cost = ledger.entries.get(&id).map_or(0.0, |entry| entry.cost_usd);
        rows.push(RepriceRow {
            event_id: id.clone(),
            model: model.to_owned(),
            price_revision: revision.clone(),
            rate: price,
            previous_cost_usd: previous_cost,
            effective_cost_usd: cost,
            delta_usd: cost - previous_cost,
        });
        next.insert(
            id,
            Correction {
                raw_sha256: raw.hash,
                price_revision: revision,
                rate: price,
                categories,
                cost_usd: cost,
                day: raw.event.day().to_owned(),
                model: model.to_owned(),
                ticket: raw.event.ticket,
            },
        );
    }
    Ok((rows, next))
}

/// Preview the effective per-model rate and replacement delta without changing local files.
pub fn preview(store: &FsStore) -> io::Result<RepriceReport> {
    let ledger = load_ledger(store)?;
    let (rows, next) = candidates(store, &ledger)?;
    let changed = next
        .iter()
        .filter(|(id, entry)| {
            ledger.entries.get(*id).is_none_or(|old| {
                old.price_revision != entry.price_revision || old.raw_sha256 != entry.raw_sha256
            })
        })
        .count();
    Ok(RepriceReport { rows, changed })
}

/// Retention cannot discard a raw event whose currently effective rate has not
/// been applied to the settled projection yet.
pub(super) fn pending_before(store: &FsStore, before_day: &str) -> io::Result<bool> {
    let ledger = load_ledger(store)?;
    let (_, next) = candidates(store, &ledger)?;
    Ok(next.iter().any(|(id, entry)| {
        entry.day.as_str() < before_day
            && ledger.entries.get(id).is_none_or(|previous| {
                previous.price_revision != entry.price_revision
                    || previous.raw_sha256 != entry.raw_sha256
            })
    }))
}

/// Apply once per raw identity and effective price revision. Rebuilding a damaged ledger
/// is explicit and safe only while all settled raw evidence remains available.
pub fn apply(store: &FsStore, rebuild: bool) -> io::Result<RepriceReport> {
    let _lock = lock(store)?;
    let mut ledger = if rebuild {
        let settled = super::read_settled(store, &super::contributor_id(store))?;
        if settled.pruned_before.is_some() {
            let expected = settled.checkpoint_sha256.as_deref().ok_or_else(|| {
                invalid("cannot rebuild correction ledger after raw usage was pruned without a checkpoint")
            })?;
            let slot = settled
                .checkpoint_slot
                .ok_or_else(|| invalid("correction checkpoint slot missing"))?;
            load_checkpoint(store, slot, expected)?
        } else {
            Ledger::default()
        }
    } else {
        load_ledger(store)?
    };
    let (rows, next) = candidates(store, &ledger)?;
    let changed = next
        .iter()
        .filter(|(id, entry)| {
            ledger.entries.get(*id).is_none_or(|old| {
                old.price_revision != entry.price_revision || old.raw_sha256 != entry.raw_sha256
            })
        })
        .count();
    if changed > 0 || rebuild {
        ledger.version = LEDGER_VERSION;
        ledger.entries.extend(next);
        let bytes = serde_json::to_vec_pretty(&ledger).map_err(io::Error::other)?;
        atomic_write(&ledger_path(store), &bytes)?;
    }
    super::refresh_settled_projection(store, &ledger.entries)?;
    Ok(RepriceReport { rows, changed })
}

pub(super) fn project_raw(store: &FsStore) -> io::Result<Vec<UsageEvent>> {
    let corrections = corrected_costs(store)?;
    project_raw_with(store, &corrections)
}

pub(super) fn project_raw_with(
    store: &FsStore,
    corrections: &BTreeMap<String, Correction>,
) -> io::Result<Vec<UsageEvent>> {
    Ok(project_raw_with_ids(store, corrections)?
        .into_iter()
        .map(|(event, _)| event)
        .collect())
}

pub(super) fn project_raw_with_ids(
    store: &FsStore,
    corrections: &BTreeMap<String, Correction>,
) -> io::Result<Vec<(UsageEvent, Option<String>)>> {
    let mut events = Vec::new();
    let mut seen = BTreeSet::new();
    for mut raw in read_evidence(store)? {
        if let Some(id) = raw.id.as_deref() {
            if !seen.insert(id.to_owned()) {
                return Err(invalid(format!("duplicate raw event identity: {id}")));
            }
            if let Some(correction) = corrections.get(id) {
                if correction.raw_sha256 != raw.hash {
                    return Err(invalid(format!("raw evidence changed for event {id}")));
                }
                if raw.event.cost_usd.is_some()
                    || raw.event.tool != "claude"
                    || raw.categories != Some(correction.categories)
                    || raw.event.model.as_deref() != Some(correction.model.as_str())
                    || raw.event.day() != correction.day
                    || raw.event.ticket != correction.ticket
                {
                    return Err(invalid(format!("correction does not match raw event {id}")));
                }
                raw.event.cost_usd = Some(correction.cost_usd);
            }
        }
        events.push((raw.event, raw.id));
    }
    Ok(events)
}

pub(super) fn project_settled(
    base: &Rollup,
    through: &str,
    corrections: &BTreeMap<String, Correction>,
) -> Rollup {
    let mut total = base.clone();
    for correction in corrections
        .values()
        .filter(|correction| correction.day.as_str() <= through)
    {
        let cost = correction.cost_usd;
        total.cost_usd += cost;
        *total.by_model.entry(correction.model.clone()).or_default() += cost;
        if let Some(ticket) = &correction.ticket {
            *total.by_ticket.entry(ticket.clone()).or_default() += cost;
        }
        *total.by_day.entry(correction.day.clone()).or_default() += cost;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{
        ClaudeTokenCategories, record_once_with_claude_tokens, roll_up_through, summary,
        summary_settled, team_summary, ticket_usage,
    };
    use crate::store::StoreMetadata;
    use std::sync::Arc;

    fn fixture() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        (dir, store)
    }

    fn event(model: Option<&str>, input: u64, output: u64) -> UsageEvent {
        UsageEvent {
            ts: "2026-10-22T12:00:00Z".into(),
            tool: "claude".into(),
            model: model.map(str::to_owned),
            tokens_in: input,
            tokens_out: output,
            cost_usd: None,
            ticket: Some("ticket-1".into()),
            session: Some("session-1".into()),
        }
    }

    fn categories() -> ClaudeTokenCategories {
        ClaudeTokenCategories {
            uncached_input: 1_000_000,
            cache_read: 1_000_000,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_unclassified: 0,
            output: 0,
        }
    }

    fn raw_path(store: &FsStore) -> PathBuf {
        store.root().join("metrics/raw/2026-10-22.jsonl")
    }

    #[test]
    fn hs2_38saet_preview_apply_revision_and_settled_projection() {
        let (_dir, store) = fixture();
        record_once_with_claude_tokens(
            &store,
            &event(Some("claude-fable-5-1"), 2_000_000, 0),
            "eligible",
            Some(categories()),
        )
        .unwrap();
        record_once_with_claude_tokens(
            &store,
            &event(Some("claude-fable-5-1"), 2_000_000, 0),
            "aggregate-only",
            None,
        )
        .unwrap();
        record_once_with_claude_tokens(
            &store,
            &event(None, 2_000_000, 0),
            "mixed-model",
            Some(categories()),
        )
        .unwrap();
        let mut unclassified = categories();
        unclassified.cache_read -= 1;
        unclassified.cache_write_unclassified = 1;
        record_once_with_claude_tokens(
            &store,
            &event(Some("claude-fable-5-1"), 2_000_000, 0),
            "unclassified",
            Some(unclassified),
        )
        .unwrap();
        let raw_before = fs::read(raw_path(&store)).unwrap();

        let initial_preview = preview(&store).unwrap();
        assert_eq!(initial_preview.rows.len(), 1);
        assert_eq!(initial_preview.changed, 1);
        assert_eq!(initial_preview.rows[0].event_id, "eligible");
        assert!((initial_preview.rows[0].delta_usd - 10.25).abs() < 1e-9);
        assert!(!ledger_path(&store).exists(), "preview is read-only");

        roll_up_through(&store, "2026-10-23").unwrap();
        assert_eq!(summary_settled(&store).unwrap().cost_usd, 0.0);
        assert!(
            super::super::prune_raw_before(&store, "2026-10-23").is_err(),
            "retention waits for the explicit correction"
        );
        assert_eq!(apply(&store, false).unwrap().changed, 1);
        assert_eq!(apply(&store, false).unwrap().changed, 0);
        assert_eq!(fs::read(raw_path(&store)).unwrap(), raw_before);
        assert!((summary(&store).unwrap().cost_usd - 10.25).abs() < 1e-9);
        assert!((summary_settled(&store).unwrap().cost_usd - 10.25).abs() < 1e-9);
        assert!((team_summary(&store).unwrap().cost_usd - 10.25).abs() < 1e-9);
        let usage = ticket_usage(&store, "ticket-1").unwrap();
        assert!((usage.settled_cost_usd - 10.25).abs() < 1e-9);
        assert_eq!(usage.events[0].cost_usd, Some(10.25));
        roll_up_through(&store, "2026-10-23").unwrap();
        assert!((summary_settled(&store).unwrap().cost_usd - 10.25).abs() < 1e-9);
        assert!(roll_up_through(&store, "2026-10-22").is_err());

        let path = store.root().join("metrics/prices.json");
        fs::write(
            path,
            r#"{"claude-fable-5-1":{"input_per_mtok":20,"output_per_mtok":50,"cache_read_per_mtok":0.5,"cache_write_5m_per_mtok":12.5,"cache_write_1h_per_mtok":20}}"#,
        )
        .unwrap();
        let revised = preview(&store).unwrap();
        assert_eq!(revised.changed, 1);
        assert_ne!(
            revised.rows[0].price_revision,
            initial_preview.rows[0].price_revision
        );
        assert!((revised.rows[0].delta_usd - 10.25).abs() < 1e-9);
        assert_eq!(apply(&store, false).unwrap().changed, 1);
        assert!((summary_settled(&store).unwrap().cost_usd - 20.5).abs() < 1e-9);
        assert!((ticket_usage(&store, "ticket-1").unwrap().settled_cost_usd - 20.5).abs() < 1e-9);
        assert_eq!(fs::read(raw_path(&store)).unwrap(), raw_before);

        // An interrupted apply may leave the rollup's stored projection stale. The
        // uncorrected base and ledger still determine a correct live and team view.
        let rollup_path = super::super::rollups_dir(&store)
            .join(super::super::contributor_id(&store))
            .join("rollup.json");
        let mut stale: serde_json::Value =
            serde_json::from_slice(&fs::read(&rollup_path).unwrap()).unwrap();
        stale["cost_usd"] = 0.0.into();
        fs::write(&rollup_path, serde_json::to_vec(&stale).unwrap()).unwrap();
        assert!((summary_settled(&store).unwrap().cost_usd - 20.5).abs() < 1e-9);
        assert!((team_summary(&store).unwrap().cost_usd - 20.5).abs() < 1e-9);
    }

    #[test]
    fn hs2_38saet_concurrent_apply_and_corrupt_ledger_rebuild() {
        let (_dir, store) = fixture();
        record_once_with_claude_tokens(
            &store,
            &event(Some("claude-fable-5-1"), 2_000_000, 0),
            "eligible",
            Some(categories()),
        )
        .unwrap();
        let store = Arc::new(store);
        let workers = (0..4)
            .map(|_| {
                let store = Arc::clone(&store);
                std::thread::spawn(move || apply(&store, false).unwrap().changed)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .sum::<usize>(),
            1
        );
        assert_eq!(summary(&store).unwrap().cost_usd, 10.25);
        fs::write(ledger_path(&store), "{broken").unwrap();
        assert!(preview(&store).is_err());
        assert!(summary(&store).is_err());
        assert!(apply(&store, false).is_err());
        assert_eq!(apply(&store, true).unwrap().changed, 1);
        assert_eq!(summary(&store).unwrap().cost_usd, 10.25);

        roll_up_through(&store, "2026-10-23").unwrap();
        let rollup_path = super::super::rollups_dir(&store)
            .join(super::super::contributor_id(&store))
            .join("rollup.json");
        let valid_rollup = fs::read(&rollup_path).unwrap();
        fs::write(&rollup_path, "{broken").unwrap();
        assert!(summary_settled(&store).is_err());
        assert!(apply(&store, false).is_err());
        assert_eq!(fs::read(&rollup_path).unwrap(), b"{broken");
        fs::write(&rollup_path, valid_rollup).unwrap();
        super::super::prune_raw_before(&store, "2026-10-23").unwrap();
        assert_eq!(summary_settled(&store).unwrap().cost_usd, 10.25);
        fs::write(ledger_path(&store), "{broken").unwrap();
        assert!(summary(&store).is_err());
        assert_eq!(apply(&store, true).unwrap().changed, 0);
        assert_eq!(summary_settled(&store).unwrap().cost_usd, 10.25);
        assert_eq!(team_summary(&store).unwrap().cost_usd, 10.25);
    }

    #[test]
    fn hs2_5e8vy5_checkpoint_recovers_after_pruning_and_rejects_corruption() {
        let (_dir, store) = fixture();
        record_once_with_claude_tokens(
            &store,
            &event(Some("claude-fable-5-1"), 2_000_000, 0),
            "settled-turn",
            Some(categories()),
        )
        .unwrap();
        apply(&store, false).unwrap();
        roll_up_through(&store, "2026-10-23").unwrap();
        assert_eq!(
            super::super::prune_raw_before(&store, "2026-10-23").unwrap(),
            1
        );
        let rollup_path = super::super::rollups_dir(&store)
            .join(super::super::contributor_id(&store))
            .join("rollup.json");
        let settled: serde_json::Value =
            serde_json::from_slice(&fs::read(&rollup_path).unwrap()).unwrap();
        let slot = settled["checkpoint_slot"].as_u64().unwrap() as u8;
        let checkpoint = fs::read(checkpoint_path(&store, slot)).unwrap();
        assert_eq!(settled["checkpoint_sha256"], digest(&checkpoint));

        // A missing primary is an error, including for ordinary readers, until
        // an explicit rebuild verifies the checkpoint and restores it.
        fs::remove_file(ledger_path(&store)).unwrap();
        assert!(summary(&store).is_err());
        assert_eq!(apply(&store, true).unwrap().changed, 0);
        assert_eq!(summary_settled(&store).unwrap().cost_usd, 10.25);
        assert_eq!(team_summary(&store).unwrap().cost_usd, 10.25);
        assert_eq!(
            ticket_usage(&store, "ticket-1").unwrap().settled_cost_usd,
            10.25
        );

        // A partial or stale backup must not replace a damaged primary.
        fs::write(ledger_path(&store), "{broken").unwrap();
        fs::write(
            checkpoint_path(&store, slot),
            &checkpoint[..checkpoint.len() / 2],
        )
        .unwrap();
        assert!(apply(&store, true).is_err());
        assert_eq!(fs::read(ledger_path(&store)).unwrap(), b"{broken");
        fs::write(checkpoint_path(&store, slot), &checkpoint).unwrap();
        let mut wrong_marker = settled.clone();
        wrong_marker["checkpoint_sha256"] = "0".repeat(64).into();
        fs::write(&rollup_path, serde_json::to_vec(&wrong_marker).unwrap()).unwrap();
        assert!(apply(&store, true).is_err());
        fs::write(&rollup_path, serde_json::to_vec(&settled).unwrap()).unwrap();
        assert_eq!(apply(&store, true).unwrap().changed, 0);
        assert_eq!(summary_settled(&store).unwrap().cost_usd, 10.25);
    }

    #[test]
    fn hs2_5e8vy5_interrupted_prune_keeps_previous_checkpoint() {
        let (_dir, store) = fixture();
        record_once_with_claude_tokens(
            &store,
            &event(Some("claude-fable-5-1"), 2_000_000, 0),
            "pruned-turn",
            Some(categories()),
        )
        .unwrap();
        apply(&store, false).unwrap();
        roll_up_through(&store, "2026-10-23").unwrap();
        super::super::prune_raw_before(&store, "2026-10-23").unwrap();
        let rollup_path = super::super::rollups_dir(&store)
            .join(super::super::contributor_id(&store))
            .join("rollup.json");
        let settled: serde_json::Value =
            serde_json::from_slice(&fs::read(&rollup_path).unwrap()).unwrap();
        let active = settled["checkpoint_slot"].as_u64().unwrap() as u8;
        let inactive = 1 - active;

        // Simulate a second prune interrupted after replacing its inactive
        // checkpoint but before committing the new settled marker.
        checkpoint(&store, inactive).unwrap();
        fs::write(checkpoint_path(&store, inactive), b"{partial").unwrap();
        fs::write(ledger_path(&store), b"{broken").unwrap();
        assert_eq!(apply(&store, true).unwrap().changed, 0);
        assert_eq!(summary_settled(&store).unwrap().cost_usd, 10.25);

        // A completed repeat prune switches slots. The old slot can now be
        // damaged without breaking recovery from the newly committed one.
        assert_eq!(
            super::super::prune_raw_before(&store, "2026-10-23").unwrap(),
            0
        );
        let next: serde_json::Value =
            serde_json::from_slice(&fs::read(&rollup_path).unwrap()).unwrap();
        assert_eq!(next["checkpoint_slot"].as_u64(), Some(u64::from(inactive)));
        fs::write(checkpoint_path(&store, active), b"{partial").unwrap();
        fs::write(ledger_path(&store), b"{broken").unwrap();
        assert_eq!(apply(&store, true).unwrap().changed, 0);
        assert_eq!(team_summary(&store).unwrap().cost_usd, 10.25);
    }

    #[test]
    fn hs2_5e8vy5_rebuild_combines_checkpoint_with_retained_tail() {
        let (_dir, store) = fixture();
        record_once_with_claude_tokens(
            &store,
            &event(Some("claude-fable-5-1"), 2_000_000, 0),
            "pruned-turn",
            Some(categories()),
        )
        .unwrap();
        apply(&store, false).unwrap();
        roll_up_through(&store, "2026-10-23").unwrap();
        super::super::prune_raw_before(&store, "2026-10-23").unwrap();

        let mut tail = event(Some("claude-fable-5-1"), 2_000_000, 0);
        tail.ts = "2026-10-24T12:00:00Z".into();
        record_once_with_claude_tokens(&store, &tail, "tail-turn", Some(categories())).unwrap();
        assert_eq!(apply(&store, false).unwrap().changed, 1);
        roll_up_through(&store, "2026-10-24").unwrap();
        fs::write(ledger_path(&store), "{broken").unwrap();
        assert_eq!(apply(&store, true).unwrap().changed, 1);
        assert_eq!(summary_settled(&store).unwrap().cost_usd, 20.5);
        assert_eq!(team_summary(&store).unwrap().cost_usd, 20.5);
        assert_eq!(
            ticket_usage(&store, "ticket-1").unwrap().settled_cost_usd,
            20.5
        );
    }

    #[test]
    fn hs2_5e8vy5_legacy_prune_without_checkpoint_stays_fail_closed() {
        let (_dir, store) = fixture();
        record_once_with_claude_tokens(
            &store,
            &event(Some("claude-fable-5-1"), 2_000_000, 0),
            "pruned-turn",
            Some(categories()),
        )
        .unwrap();
        apply(&store, false).unwrap();
        roll_up_through(&store, "2026-10-23").unwrap();
        super::super::prune_raw_before(&store, "2026-10-23").unwrap();
        let rollup_path = super::super::rollups_dir(&store)
            .join(super::super::contributor_id(&store))
            .join("rollup.json");
        let mut settled: serde_json::Value =
            serde_json::from_slice(&fs::read(&rollup_path).unwrap()).unwrap();
        settled.as_object_mut().unwrap().remove("checkpoint_sha256");
        fs::write(&rollup_path, serde_json::to_vec(&settled).unwrap()).unwrap();
        fs::write(ledger_path(&store), "{broken").unwrap();
        assert!(apply(&store, true).is_err());
        assert!(summary_settled(&store).is_err());
    }

    #[test]
    fn hs2_38saet_distinct_turn_ids_with_equal_counters_stay_distinct_in_ticket_history() {
        let (_dir, store) = fixture();
        let usage = event(Some("claude-fable-5-1"), 2_000_000, 0);
        for id in ["turn-1", "turn-2"] {
            record_once_with_claude_tokens(&store, &usage, id, Some(categories())).unwrap();
        }
        assert_eq!(apply(&store, false).unwrap().changed, 2);
        let history = ticket_usage(&store, "ticket-1").unwrap();
        assert_eq!(history.events.len(), 2);
        assert_eq!(history.events[0].cost_usd, Some(10.25));
        assert_eq!(history.events[1].cost_usd, Some(10.25));
        assert_eq!(summary(&store).unwrap().cost_usd, 20.5);
    }
}
