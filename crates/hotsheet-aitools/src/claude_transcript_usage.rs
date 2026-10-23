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

fn usage_from_lines(bytes: &[u8], prices: Option<&Prices>) -> Option<ClaudeTurnUsage> {
    let mut messages =
        BTreeMap::<String, (String, Usage, Option<f64>, Option<ClaudeTokenCategories>)>::new();
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
            .is_none_or(|(previous, _, _, _)| timestamp >= previous.as_str())
        {
            messages.insert(
                id.to_owned(),
                (timestamp.to_owned(), usage, cost, categories),
            );
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
