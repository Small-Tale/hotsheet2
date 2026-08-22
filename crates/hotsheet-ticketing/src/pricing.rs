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
}

impl Price {
    /// The cost of `tokens_in` input + `tokens_out` output tokens at this price.
    pub fn cost(&self, tokens_in: u64, tokens_out: u64) -> f64 {
        (tokens_in as f64 * self.input_per_mtok + tokens_out as f64 * self.output_per_mtok) / 1e6
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
    };
    BTreeMap::from([
        ("claude-opus-4-8".into(), p(15.0, 75.0)),
        ("claude-sonnet-5".into(), p(3.0, 15.0)),
        ("claude-haiku-4-5".into(), p(0.80, 4.0)),
    ])
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
    prices.get(model).map(|p| p.cost(tokens_in, tokens_out))
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
