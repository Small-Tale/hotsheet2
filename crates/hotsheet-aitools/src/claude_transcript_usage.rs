//! Numeric usage from the new portion of one interactive Claude transcript.
//!
//! Claude's documented turn hooks provide the transcript path but no token counters. The
//! JSONL row shape is not a hook contract, so unknown or malformed rows are ignored and no
//! conversation content is retained. Repeated rows for one assistant message count once.

use crate::drive::Usage;
use hotsheet_ticketing::pricing::Prices;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

const MAX_TURN_BYTES: u64 = 64 * 1024 * 1024;

/// Read only bytes appended after a `UserPromptSubmit` hook's file-size snapshot.
/// A truncated/replaced transcript or an unbounded turn produces no guessed usage.
pub fn usage_since(path: &Path, offset: u64) -> io::Result<Option<Usage>> {
    usage_since_with_prices(path, offset, None)
}

/// Price the transcript's separate cache categories instead of treating every input
/// token as an uncached input token.
pub fn priced_usage_since(path: &Path, offset: u64, prices: &Prices) -> io::Result<Option<Usage>> {
    usage_since_with_prices(path, offset, Some(prices))
}

fn usage_since_with_prices(
    path: &Path,
    offset: u64,
    prices: Option<&Prices>,
) -> io::Result<Option<Usage>> {
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

fn usage_from_lines(bytes: &[u8], prices: Option<&Prices>) -> Option<Usage> {
    let mut messages = BTreeMap::<String, (String, Usage, Option<f64>)>::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("assistant")
            || value.get("isSidechain").and_then(Value::as_bool) == Some(true)
            || value
                .get("parent_tool_use_id")
                .is_some_and(|parent| !parent.is_null())
        {
            continue;
        }
        let Some(message) = value.get("message") else {
            continue;
        };
        let Some(id) = message.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(usage) = crate::claude::claude_result_usage(message) else {
            continue;
        };
        let cost = prices.and_then(|prices| {
            let model = usage.model.as_ref()?;
            let price = prices.get(model)?;
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
                count("cache_creation_input_tokens")
                    .saturating_sub(write_5m.saturating_add(write_1h)),
                count("output_tokens"),
            )
        });
        if usage.tokens_in == 0 && usage.tokens_out == 0 {
            continue;
        }
        let timestamp = value.get("timestamp").and_then(Value::as_str).unwrap_or("");
        // A streamed assistant message may appear on several rows. The latest snapshot of
        // its counters is authoritative; stale rows received out of order cannot lower it.
        if messages
            .get(id)
            .is_none_or(|(previous, _, _)| timestamp >= previous.as_str())
        {
            messages.insert(id.to_owned(), (timestamp.to_owned(), usage, cost));
        }
    }
    if messages.is_empty() {
        return None;
    }
    let mut total = Usage::default();
    let mut model = None::<String>;
    let mut mixed_models = false;
    let mut total_cost = prices.map(|_| 0.0);
    for (_, usage, cost) in messages.values() {
        total.tokens_in = total.tokens_in.saturating_add(usage.tokens_in);
        total.tokens_out = total.tokens_out.saturating_add(usage.tokens_out);
        total_cost = total_cost.zip(*cost).map(|(sum, amount)| sum + amount);
        if let Some(current) = &usage.model {
            if model.as_ref().is_some_and(|known| known != current) {
                mixed_models = true;
            } else {
                model = Some(current.clone());
            }
        }
    }
    total.model = if mixed_models { None } else { model };
    total.cost_usd = total_cost;
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
}
