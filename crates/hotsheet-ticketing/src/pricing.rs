//! Per-model **price table** (`docs/14` §14.2/§14.6) — so a usage event's `cost_usd` is
//! always present even when a tool doesn't report it. Ships a small default table; a store
//! may override / extend it with a committed `metrics/prices.json` (the maintainer updates
//! prices as models change; the `claude-api` skill is the source of truth for Anthropic).
//!
//! Prices are **USD per million tokens** — the unit vendors publish — so the numbers in the
//! table read like the price sheets.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::store::FsStore;

/// A model's input/output price, in **USD per million tokens**.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Price {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    #[serde(default)]
    pub cache_read_per_mtok: Option<f64>,
    #[serde(default)]
    pub cache_write_5m_per_mtok: Option<f64>,
    #[serde(default)]
    pub cache_write_1h_per_mtok: Option<f64>,
}

impl Price {
    /// The cost of `tokens_in` input + `tokens_out` output tokens at this price.
    pub fn cost(&self, tokens_in: u64, tokens_out: u64) -> f64 {
        (tokens_in as f64 * self.input_per_mtok + tokens_out as f64 * self.output_per_mtok) / 1e6
    }

    /// Price Claude's separately reported input categories when all needed rates exist.
    /// An unclassified cache write cannot be priced without guessing its TTL.
    pub fn claude_cost(
        &self,
        uncached: u64,
        cache_read: u64,
        cache_write_5m: u64,
        cache_write_1h: u64,
        unclassified_cache_write: u64,
        output: u64,
    ) -> Option<f64> {
        if unclassified_cache_write > 0 {
            return None;
        }
        let read = if cache_read == 0 {
            0.0
        } else {
            self.cache_read_per_mtok?
        };
        let write_5m = if cache_write_5m == 0 {
            0.0
        } else {
            self.cache_write_5m_per_mtok?
        };
        let write_1h = if cache_write_1h == 0 {
            0.0
        } else {
            self.cache_write_1h_per_mtok?
        };
        Some(
            (uncached as f64 * self.input_per_mtok
                + cache_read as f64 * read
                + cache_write_5m as f64 * write_5m
                + cache_write_1h as f64 * write_1h
                + output as f64 * self.output_per_mtok)
                / 1e6,
        )
    }
}

/// A model → price map.
pub type Prices = BTreeMap<String, Price>;

/// The built-in default prices (a starting point; override via `metrics/prices.json`). Keep
/// this small — it's a convenience, not an exhaustive registry.
pub fn default_prices() -> Prices {
    let p = |i: f64, o: f64| Price {
        input_per_mtok: i,
        output_per_mtok: o,
        cache_read_per_mtok: None,
        cache_write_5m_per_mtok: None,
        cache_write_1h_per_mtok: None,
    };
    let mut prices = BTreeMap::from([
        ("claude-opus-4-8".into(), p(15.0, 75.0)),
        ("claude-sonnet-5".into(), p(3.0, 15.0)),
        ("claude-haiku-4-5".into(), p(0.80, 4.0)),
    ]);
    prices.insert(
        "claude-fable-5-1".into(),
        Price {
            input_per_mtok: 10.0,
            output_per_mtok: 50.0,
            cache_read_per_mtok: Some(0.25),
            cache_write_5m_per_mtok: Some(12.50),
            cache_write_1h_per_mtok: Some(20.0),
        },
    );
    prices
}

/// The store's effective prices: the defaults, overlaid by a committed `metrics/prices.json`
/// (`{ "model": { "input_per_mtok": .., "output_per_mtok": .. }, … }`) when present. A
/// missing or malformed override file leaves the defaults.
pub fn load_prices(store: &FsStore) -> Prices {
    let mut prices = default_prices();
    let path = store.root().join("metrics").join("prices.json");
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Ok(over) = serde_json::from_str::<Prices>(&text) {
            prices.extend(over);
        }
    }
    prices
}

/// The cost of a usage at `model` per `prices`, or `None` when the model isn't priced.
pub fn cost(prices: &Prices, model: &str, tokens_in: u64, tokens_out: u64) -> Option<f64> {
    prices.get(model).and_then(|price| {
        // An aggregate input count cannot distinguish cache reads and writes. Models
        // with category-specific rates need those counters (as the Claude transcript
        // path supplies) before assigning a cost.
        if price.cache_read_per_mtok.is_some()
            || price.cache_write_5m_per_mtok.is_some()
            || price.cache_write_1h_per_mtok.is_some()
        {
            None
        } else {
            Some(price.cost(tokens_in, tokens_out))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoreMetadata;

    #[test]
    fn cost_is_per_million_tokens() {
        let price = Price {
            input_per_mtok: 15.0,
            output_per_mtok: 75.0,
            cache_read_per_mtok: None,
            cache_write_5m_per_mtok: None,
            cache_write_1h_per_mtok: None,
        };
        // 1M in + 1M out = 15 + 75 = $90.
        assert!((price.cost(1_000_000, 1_000_000) - 90.0).abs() < 1e-9);
        // 100k in + 20k out at opus = 1.5 + 1.5 = $3.00.
        assert!((price.cost(100_000, 20_000) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn unknown_model_has_no_price() {
        let prices = default_prices();
        assert!(cost(&prices, "claude-opus-4-8", 1000, 100).is_some());
        assert!(cost(&prices, "some-unlisted-model", 1000, 100).is_none());
    }

    #[test]
    fn fable_prices_cached_inputs_at_their_own_rates() {
        let price = default_prices()["claude-fable-5-1"];
        assert_eq!(
            price.claude_cost(1_000_000, 1_000_000, 1_000_000, 1_000_000, 0, 1_000_000),
            Some(92.75)
        );
        assert_eq!(price.claude_cost(0, 0, 0, 0, 1, 0), None);
        assert_eq!(
            cost(&default_prices(), "claude-fable-5-1", 1_000_000, 0),
            None
        );
    }

    #[test]
    fn a_committed_prices_file_overrides_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        std::fs::create_dir_all(dir.path().join("metrics")).unwrap();
        std::fs::write(
            dir.path().join("metrics/prices.json"),
            r#"{ "claude-opus-4-8": { "input_per_mtok": 10.0, "output_per_mtok": 50.0 },
                 "new-model": { "input_per_mtok": 1.0, "output_per_mtok": 2.0 } }"#,
        )
        .unwrap();

        let prices = load_prices(&store);
        // The override wins for opus…
        assert!((cost(&prices, "claude-opus-4-8", 1_000_000, 0).unwrap() - 10.0).abs() < 1e-9);
        // …a new model is added…
        assert!((cost(&prices, "new-model", 1_000_000, 0).unwrap() - 1.0).abs() < 1e-9);
        // …and an un-overridden default stays.
        assert!(cost(&prices, "claude-sonnet-5", 1_000_000, 0).is_some());
    }
}
