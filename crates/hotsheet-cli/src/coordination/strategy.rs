//! Integration strategies (HS2-NEY1QH).
//!
//! The integrator runs under exactly one strategy per session generation. `serial` is
//! the default and today's behavior: an exact-tree affected gate, then a push, one
//! integration at a time. Other strategies are registered here so their tickets plug in
//! without touching every call site, but this executable refuses any strategy whose
//! descriptor is not `implemented`.
//!
//! A project allows strategies in its committed shared settings
//! ([`STRATEGY_POLICY_SETTING`]); every non-default strategy carries a recorded reason,
//! because such strategies deviate from shared rules such as `push-per-ticket`.

use anyhow::{Result, bail, ensure};
use hotsheet_ticketing::{Scope, Settings};
use serde::{Deserialize, Serialize};

/// Shared project settings key holding the [`StrategyPolicy`].
pub const STRATEGY_POLICY_SETTING: &str = "integration_strategies";

/// Longest accepted allow reason, in characters.
pub const MAX_REASON_CHARS: usize = 500;

/// The typed registry of integration strategies. Adding a strategy means adding a
/// variant, its [`StrategyDescriptor`] and its [`StrategyRules`]; call sites consult
/// those rather than matching on names.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
pub enum IntegrationStrategy {
    /// Per-ticket exact-tree affected gate, then push; one integration at a time.
    #[default]
    Serial,
    /// One smoke gate and one push for a batch of handoffs (HS2-9G0M3V).
    Batch,
    /// Batches staged on an integration branch; main fast-forwards to green (HS2-GT57BQ).
    CiStaged,
}

/// Metadata reported by `coordination capabilities` and `coordination strategies`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct StrategyDescriptor {
    pub name: &'static str,
    /// Whether this executable can run an integrator under the strategy.
    pub implemented: bool,
    /// The ticket that owns the strategy's behavior.
    pub ticket: &'static str,
    pub summary: &'static str,
}

/// Behavior switches the coordination runtime consults. Only rules some code path reads
/// belong here; a later strategy adds the rule together with the code that reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct StrategyRules {
    /// At most one assignment may be integrating or verifying at a time through `mark`
    /// and `resume-integration`. A batch opened atomically counts as one integration unit.
    pub exclusive_integration: bool,
    /// The integrator may open one batch of several integrating code handoffs, smoke-gate
    /// and push it once, and let CI test it together (HS2-9G0M3V).
    pub batch_integration: bool,
}

impl IntegrationStrategy {
    pub const ALL: [Self; 3] = [Self::Serial, Self::Batch, Self::CiStaged];

    pub const fn descriptor(self) -> StrategyDescriptor {
        match self {
            Self::Serial => StrategyDescriptor {
                name: "serial",
                implemented: true,
                ticket: "HS2-NEY1QH",
                summary: "Integrate one handoff at a time: exact-tree affected gate, push, then CI per ticket.",
            },
            Self::Batch => StrategyDescriptor {
                name: "batch",
                implemented: true,
                ticket: "HS2-9G0M3V",
                summary: "Integrate several handoffs, run one smoke gate, push once and let CI test the batch.",
            },
            Self::CiStaged => StrategyDescriptor {
                name: "ci-staged",
                implemented: false,
                ticket: "HS2-GT57BQ",
                summary: "Stage batches on an integration branch and fast-forward main only to a green SHA.",
            },
        }
    }

    pub const fn rules(self) -> StrategyRules {
        match self {
            // Unimplemented strategies keep serial rules until their tickets change them.
            Self::Serial | Self::CiStaged => StrategyRules {
                exclusive_integration: true,
                batch_integration: false,
            },
            Self::Batch => StrategyRules {
                exclusive_integration: true,
                batch_integration: true,
            },
        }
    }

    pub const fn name(self) -> &'static str {
        self.descriptor().name
    }

    pub fn is_serial(&self) -> bool {
        *self == Self::Serial
    }

    pub fn parse(raw: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|strategy| strategy.name() == raw)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "unknown integration strategy {raw}; known strategies: {}",
                    Self::ALL.map(Self::name).join(", ")
                )
            })
    }
}

impl std::fmt::Display for IntegrationStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Every registered strategy's descriptor, in registry order.
pub const REGISTRY: [StrategyDescriptor; 3] = [
    IntegrationStrategy::Serial.descriptor(),
    IntegrationStrategy::Batch.descriptor(),
    IntegrationStrategy::CiStaged.descriptor(),
];

/// A non-default strategy the project explicitly allows, with the recorded reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllowedStrategy {
    pub strategy: IntegrationStrategy,
    pub reason: String,
    /// Batch only: when `main` stays red this long, the integrator falls back to serial
    /// (HS2-9G0M3V). Absent means [`CircuitBreaker::default`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circuit_breaker: Option<CircuitBreaker>,
}

/// Batch circuit breaker thresholds (HS2-9G0M3V). Either limit trips the breaker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CircuitBreaker {
    /// Consecutive red batches that trip the breaker (1-100).
    pub max_red_batches: u32,
    /// Minutes `main` may stay red after the first red batch (1-10080).
    pub max_red_minutes: u32,
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self {
            max_red_batches: 2,
            max_red_minutes: 120,
        }
    }
}

impl CircuitBreaker {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=100).contains(&self.max_red_batches),
            "circuit breaker max_red_batches must be 1-100"
        );
        ensure!(
            (1..=10_080).contains(&self.max_red_minutes),
            "circuit breaker max_red_minutes must be 1-10080"
        );
        Ok(())
    }
}

/// The project's integration strategy policy. `serial` is always allowed and needs no
/// reason; an absent setting is the serial-only default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyPolicy {
    #[serde(default)]
    pub default: IntegrationStrategy,
    #[serde(default)]
    pub allowed: Vec<AllowedStrategy>,
}

/// The policy together with the registry, as read by CLI and REST clients.
#[derive(Clone, Debug, Serialize)]
pub struct StrategyReport {
    pub policy: StrategyPolicy,
    pub registry: [StrategyDescriptor; 3],
}

impl StrategyPolicy {
    pub fn report(self) -> StrategyReport {
        StrategyReport {
            policy: self,
            registry: REGISTRY,
        }
    }

    pub fn validate(&self) -> Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        for entry in &self.allowed {
            ensure!(
                !entry.strategy.is_serial(),
                "serial is always allowed; do not list it"
            );
            ensure!(
                seen.insert(entry.strategy),
                "integration strategy {} is listed more than once",
                entry.strategy
            );
            let reason = entry.reason.trim();
            ensure!(
                !reason.is_empty() && reason.chars().count() <= MAX_REASON_CHARS,
                "allowing integration strategy {} requires a reason of 1-{MAX_REASON_CHARS} characters",
                entry.strategy
            );
            if let Some(breaker) = &entry.circuit_breaker {
                ensure!(
                    entry.strategy.rules().batch_integration,
                    "only the batch strategy has a circuit breaker"
                );
                breaker.validate()?;
            }
        }
        ensure!(
            self.is_allowed(self.default),
            "default integration strategy {} must be serial or an allowed strategy",
            self.default
        );
        Ok(())
    }

    pub fn is_allowed(&self, strategy: IntegrationStrategy) -> bool {
        strategy.is_serial() || self.allowed.iter().any(|entry| entry.strategy == strategy)
    }

    /// The batch circuit breaker thresholds: the allowed batch entry's, else the default.
    pub fn circuit_breaker(&self) -> CircuitBreaker {
        self.allowed
            .iter()
            .find(|entry| entry.strategy.rules().batch_integration)
            .and_then(|entry| entry.circuit_breaker)
            .unwrap_or_default()
    }

    /// The strategy a new integrator generation runs under: the explicit request, else the
    /// project default. Refused when the project does not allow it or this executable does
    /// not implement it.
    pub fn resolve(&self, requested: Option<IntegrationStrategy>) -> Result<IntegrationStrategy> {
        let strategy = requested.unwrap_or(self.default);
        ensure!(
            self.is_allowed(strategy),
            "integration strategy {strategy} is not allowed by this project; allow it with \
             `coordination strategy-allow --strategy {strategy} --reason <why>`"
        );
        let descriptor = strategy.descriptor();
        if !descriptor.implemented {
            bail!(
                "integration strategy {strategy} is not implemented by this executable yet ({}); use serial",
                descriptor.ticket
            );
        }
        Ok(strategy)
    }

    /// Read the committed project policy. Strategies are a project decision, so a
    /// machine-local or global value is never consulted.
    pub fn read(settings: &Settings) -> Result<Self> {
        let Some(value) = settings.get(STRATEGY_POLICY_SETTING, Scope::Shared)? else {
            return Ok(Self::default());
        };
        let policy: Self = serde_json::from_value(value).map_err(|error| {
            anyhow::anyhow!("invalid {STRATEGY_POLICY_SETTING} setting: {error}")
        })?;
        policy.validate()?;
        Ok(policy)
    }

    /// Validate and replace the committed project policy.
    pub fn write(&self, settings: &Settings) -> Result<()> {
        self.validate()?;
        let mut normalized = self.clone();
        for entry in &mut normalized.allowed {
            entry.reason = entry.reason.trim().to_owned();
        }
        normalized.allowed.sort_by_key(|entry| entry.strategy);
        settings.set(
            STRATEGY_POLICY_SETTING,
            serde_json::to_value(&normalized)?,
            Scope::Shared,
        )?;
        Ok(())
    }

    /// Allow (or re-justify) a non-default strategy.
    pub fn allow(&mut self, strategy: IntegrationStrategy, reason: &str) -> Result<()> {
        self.allow_with_breaker(strategy, reason, None)
    }

    /// Allow a strategy with batch circuit breaker thresholds. Re-allowing without
    /// thresholds keeps the ones already recorded.
    pub fn allow_with_breaker(
        &mut self,
        strategy: IntegrationStrategy,
        reason: &str,
        circuit_breaker: Option<CircuitBreaker>,
    ) -> Result<()> {
        ensure!(
            !strategy.is_serial(),
            "serial is always allowed; do not list it"
        );
        let kept = self
            .allowed
            .iter()
            .find(|entry| entry.strategy == strategy)
            .and_then(|entry| entry.circuit_breaker);
        self.allowed.retain(|entry| entry.strategy != strategy);
        self.allowed.push(AllowedStrategy {
            strategy,
            reason: reason.to_owned(),
            circuit_breaker: circuit_breaker.or(kept),
        });
        self.validate()
    }

    /// Withdraw a strategy. The current default cannot be withdrawn.
    pub fn disallow(&mut self, strategy: IntegrationStrategy) -> Result<()> {
        ensure!(
            !strategy.is_serial(),
            "serial is always allowed and cannot be withdrawn"
        );
        ensure!(
            self.default != strategy,
            "{strategy} is the project default; set another default first"
        );
        self.allowed.retain(|entry| entry.strategy != strategy);
        self.validate()
    }

    pub fn set_default(&mut self, strategy: IntegrationStrategy) -> Result<()> {
        self.default = strategy;
        self.validate()
    }
}

/// What an integrator registration asks for. `requested` is an explicit, already
/// policy-checked `--strategy`; `default` is the policy-resolved strategy for a new
/// generation. A renewal of the same generation keeps its recorded strategy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StrategyRequest {
    pub requested: Option<IntegrationStrategy>,
    pub default: IntegrationStrategy,
    /// Confirms every participant runs a schema-eight artifact before the first
    /// non-serial strategy is recorded.
    pub all_participants_upgraded: bool,
}

impl StrategyRequest {
    /// Resolve against the project policy before any state lock is taken.
    pub fn from_policy(
        policy: &StrategyPolicy,
        requested: Option<IntegrationStrategy>,
        all_participants_upgraded: bool,
    ) -> Result<Self> {
        Ok(Self {
            requested: requested.map(|s| policy.resolve(Some(s))).transpose()?,
            default: policy.resolve(None)?,
            all_participants_upgraded,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch_allowed() -> StrategyPolicy {
        StrategyPolicy {
            default: IntegrationStrategy::Serial,
            allowed: vec![AllowedStrategy {
                strategy: IntegrationStrategy::Batch,
                reason: "throughput".into(),
                circuit_breaker: None,
            }],
        }
    }

    #[test]
    fn hs2_ney1qh_registry_names_round_trip_and_only_built_strategies_are_implemented() {
        for strategy in IntegrationStrategy::ALL {
            assert_eq!(
                IntegrationStrategy::parse(strategy.name()).unwrap(),
                strategy
            );
            assert_eq!(
                serde_json::to_value(strategy).unwrap(),
                serde_json::json!(strategy.name())
            );
            assert!(strategy.rules().exclusive_integration);
            // HS2-9G0M3V: only batch may open a multi-handoff integration unit.
            assert_eq!(
                strategy.rules().batch_integration,
                strategy == IntegrationStrategy::Batch
            );
        }
        assert_eq!(
            REGISTRY
                .iter()
                .filter(|d| d.implemented)
                .map(|d| d.name)
                .collect::<Vec<_>>(),
            ["serial", "batch"]
        );
        assert!(IntegrationStrategy::parse("merge-queue").is_err());
    }

    #[test]
    fn hs2_ney1qh_default_policy_resolves_serial_and_refuses_unlisted_strategies() {
        let policy = StrategyPolicy::default();
        assert_eq!(policy.resolve(None).unwrap(), IntegrationStrategy::Serial);
        assert_eq!(
            policy.resolve(Some(IntegrationStrategy::Serial)).unwrap(),
            IntegrationStrategy::Serial
        );
        let refused = policy
            .resolve(Some(IntegrationStrategy::Batch))
            .unwrap_err()
            .to_string();
        assert!(refused.contains("not allowed by this project"), "{refused}");
    }

    #[test]
    fn hs2_ney1qh_allowed_but_unimplemented_strategy_is_refused_with_its_ticket() {
        let mut policy = StrategyPolicy::default();
        policy
            .allow(IntegrationStrategy::CiStaged, "staging")
            .unwrap();
        let refused = policy
            .resolve(Some(IntegrationStrategy::CiStaged))
            .unwrap_err()
            .to_string();
        assert!(refused.contains("not implemented"), "{refused}");
        assert!(refused.contains("HS2-GT57BQ"), "{refused}");
    }

    #[test]
    fn hs2_9g0m3v_batch_resolves_only_when_the_project_allows_it() {
        assert!(
            StrategyPolicy::default()
                .resolve(Some(IntegrationStrategy::Batch))
                .unwrap_err()
                .to_string()
                .contains("not allowed by this project")
        );
        assert_eq!(
            batch_allowed()
                .resolve(Some(IntegrationStrategy::Batch))
                .unwrap(),
            IntegrationStrategy::Batch
        );
    }

    #[test]
    fn hs2_9g0m3v_circuit_breaker_thresholds_validate_default_and_survive_rejustification() {
        let mut policy = StrategyPolicy::default();
        assert_eq!(policy.circuit_breaker(), CircuitBreaker::default());
        let custom = CircuitBreaker {
            max_red_batches: 3,
            max_red_minutes: 30,
        };
        policy
            .allow_with_breaker(IntegrationStrategy::Batch, "throughput", Some(custom))
            .unwrap();
        assert_eq!(policy.circuit_breaker(), custom);
        policy
            .allow(IntegrationStrategy::Batch, "re-justified")
            .unwrap();
        assert_eq!(policy.circuit_breaker(), custom);
        for invalid in [(0, 30), (2, 0), (101, 30), (2, 10_081)] {
            let invalid = CircuitBreaker {
                max_red_batches: invalid.0,
                max_red_minutes: invalid.1,
            };
            assert!(
                StrategyPolicy::default()
                    .allow_with_breaker(IntegrationStrategy::Batch, "x", Some(invalid))
                    .is_err()
            );
        }
        let refused = StrategyPolicy::default()
            .allow_with_breaker(IntegrationStrategy::CiStaged, "x", Some(custom))
            .unwrap_err()
            .to_string();
        assert!(refused.contains("only the batch strategy"), "{refused}");
    }

    #[test]
    fn hs2_ney1qh_policy_validation_requires_reasons_unique_entries_and_an_allowed_default() {
        let mut policy = StrategyPolicy::default();
        assert!(policy.allow(IntegrationStrategy::Batch, "  ").is_err());
        assert!(policy.allow(IntegrationStrategy::Serial, "x").is_err());
        assert!(
            policy
                .allow(
                    IntegrationStrategy::Batch,
                    &"x".repeat(MAX_REASON_CHARS + 1)
                )
                .is_err()
        );
        let mut policy = StrategyPolicy::default();
        assert!(policy.set_default(IntegrationStrategy::Batch).is_err());
        let mut policy = StrategyPolicy::default();
        policy
            .allow(IntegrationStrategy::Batch, "throughput")
            .unwrap();
        policy
            .allow(IntegrationStrategy::Batch, "re-justified")
            .unwrap();
        assert_eq!(policy.allowed.len(), 1);
        assert_eq!(policy.allowed[0].reason, "re-justified");
        policy.set_default(IntegrationStrategy::Batch).unwrap();
        assert!(policy.disallow(IntegrationStrategy::Batch).is_err());
        assert!(policy.disallow(IntegrationStrategy::Serial).is_err());
        policy.set_default(IntegrationStrategy::Serial).unwrap();
        policy.disallow(IntegrationStrategy::Batch).unwrap();
        assert!(policy.allowed.is_empty());
        let duplicate = StrategyPolicy {
            default: IntegrationStrategy::Serial,
            allowed: vec![batch_allowed().allowed[0].clone(); 2],
        };
        assert!(duplicate.validate().is_err());
    }

    #[test]
    fn hs2_ney1qh_policy_round_trips_through_shared_settings_only() {
        let root = tempfile::tempdir().unwrap();
        let settings = Settings::for_project(root.path());
        assert_eq!(
            StrategyPolicy::read(&settings).unwrap(),
            StrategyPolicy::default()
        );
        let mut policy = batch_allowed();
        policy.allowed[0].reason = "  throughput  ".into();
        policy.write(&settings).unwrap();
        assert_eq!(StrategyPolicy::read(&settings).unwrap(), batch_allowed());
        // A machine-local value never changes the committed project decision.
        settings
            .set(
                STRATEGY_POLICY_SETTING,
                serde_json::json!({"default": "serial"}),
                Scope::Local,
            )
            .unwrap();
        assert_eq!(StrategyPolicy::read(&settings).unwrap(), batch_allowed());
        settings
            .set(
                STRATEGY_POLICY_SETTING,
                serde_json::json!({"default": "batch"}),
                Scope::Shared,
            )
            .unwrap();
        assert!(StrategyPolicy::read(&settings).is_err());
        settings
            .set(
                STRATEGY_POLICY_SETTING,
                serde_json::json!({"default": "serial", "extra": 1}),
                Scope::Shared,
            )
            .unwrap();
        assert!(StrategyPolicy::read(&settings).is_err());
    }

    #[test]
    fn hs2_ney1qh_request_resolves_explicit_and_default_before_locking() {
        let request =
            StrategyRequest::from_policy(&StrategyPolicy::default(), None, false).unwrap();
        assert_eq!(request, StrategyRequest::default());
        assert!(
            StrategyRequest::from_policy(
                &StrategyPolicy::default(),
                Some(IntegrationStrategy::CiStaged),
                true
            )
            .is_err()
        );
    }
}
